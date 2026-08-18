# Kvist: The Spec-Driven AI Orchestrator

## Comprehensive User Guide, Philosophy, and Development Walk-through

Welcome to **Kvist**. This guide serves as both an architectural reference and a complete, hands-on walk-through showing how Kvist can be used to construct a complex software project from scratch using user-provided AI agents.

Kvist is built on a core thesis: **AI code generation is fast but fundamentally fragile.** Without rigorous structure, letting an LLM write code directly inside a repository leads to "vibe coding"—a state of creeping technical debt, specification drift, and cascading test failures.

Kvist solves this by enforcing a strict, double-blind development lifecycle where **human engineers act as Principal Architects** and **AI agents execute highly-scoped, atomically validated task queues.**

---

## 1. The Core Philosophy: Why Kvist?

Traditional AI tools (copilots and auto-writers) operate on a "generate and debug" loop. This model breaks down as projects scale. Kvist introduces three rigid constraints to restore software engineering rigor:

1. **Specs Over Code:** You cannot write a line of code until you have a validated specification (`SPEC.md`) and an ordered, dependency-mapped task queue (`TODOS.yaml`).
2. **Lifecycle Order Enforcement (Test-Driven Development):** Implementations _cannot_ precede tests. Reviewers _cannot_ self-certify. By grounding Kvist in a strict **Test-First (TDD) methodology**, we ensure that executable unit tests are written to prove specification contracts before any implementation begins.
   - **Isolated Component Unit Testing & Mocking:** Testing primarily occurs at the unit level for each component. Each component is validated in isolation, leveraging lightweight mocking or fakes to simulate surrounding components or external infrastructure, ensuring its specifications can be mathematically or behaviorally proven.
   - **Top-Level Integration Testing:** To ensure that separate components mesh perfectly, Kvist structures top-level integration tests at the root or orchestration layer (e.g., Component C). These stitch together actual component boundaries (e.g. network parsing, db storage, and application orchestration) and may still utilize mocking for downstream external cloud boundaries.
   - **Adversarial Fuzzing:** To guarantee that Layer 2 safety boundaries (like length limits, size bounds, or packet limits) are completely impenetrable, the test-writing phase strongly encourages establishing fuzz-testing or property-based harnesses, subjecting the implementation to thousands of randomized, adversarial inputs.
3. **Double-Blind Verification (DOCS.md vs. User Docs):** To prevent "hallucinated compliance," the developer agent implementing the code never writes the documentation. A clean-slate "Documenter" agent reverse-engineers the raw code into a documentation file, and a third "Reviewer" agent compares this file against the original `SPEC.md`.

### Differentiating User Docs vs. Component Implementation Records (`DOCS.md`)

It is crucial to differentiate between two completely different types of documentation in a software project:

- **User-Facing Documentation (Public Docs):** Usually written in a dedicated `/docs/` directory. It explains _how_ to integrate with, configure, and use a component from the perspective of an external human developer.
- **Component Implementation Records (`DOCS.md` / `IMPL.md`):** This is a private, structural file written to the component directory (physically mapped as `DOCS.md` for CLI compatibility, but logically representing `IMPL.md`). This file is **not** written for end-users. It is a reverse-engineered blueprint of the raw code—documenting every private structure, internal state machine, and algorithm observed in the source.
  - _Why they are different:_ The end-user does not care about private parsing helpers, but the double-blind "Reviewer" agent absolutely needs them to verify Layer 2/3 boundary compliance. Reserving `DOCS.md` for this private verifier role prevents double-work and keeps human-facing documentation clean.

---

## 2. High-Level Project Decomposition

To build a project with Kvist, you begin by breaking down your high-level project vision into a modular hierarchy of components, including your central business/application logic orchestrator.

```
                           ┌──────────────────────┐
                           │   Root Component     │
                           │   (ROOT_CONTRACT)    │
                           └──────────┬───────────┘
                                      │
              ┌───────────────────────┼──────────────────────┐
              ▼                       ▼                      ▼
     ┌─────────────────┐     ┌─────────────────┐    ┌─────────────────┐
     │  Component A    │     │  Component B    │    │  Component C    │
     │ (network/http)  │     │   (db/storage)  │    │ (app/orchestra) │
     └─────────────────┘     └─────────────────┘    └─────────────────┘
```

### Bridging the Gap: From Vision to Scaffolded Components

How do we take the step from a high-level project idea into concrete Kvist components?

1. **The System Vision (`VISION.md`):** The human architect drafts a high-level vision document in the project root. This describes the core features, the business logic orchestrator (**Component C**), and its dependencies on infrastructure (Component A: parser/network, Component B: database).
2. **Decomposition (The Architect Agent):** An advanced **Architect Agent** (configured with a high-level reasoning model) analyzes `VISION.md`. It proposes the optimal folder tree and the boundaries between components, saving the design blueprint.
3. **Scaffolding:** The user initializes the root via `kvist init`, then creates each recommended component directory structure using the CLI:
   ```bash
   kvist spec new src/network/http
   kvist spec new src/db/storage
   kvist spec new src/app/orchestra
   ```

---

## 3. The Collaborative Iterative Design Loop

Writing specifications and executing tasks in Kvist is an open-ended, highly interactive collaboration between the human architect and specialized AI agent roles.

```
                     ┌──────────────────────────┐
                     │     Human Architect      │
                     │  Drafts Core Spec Invar  │
                     └────────────┬─────────────┘
                                  │
                                  ▼
                     ┌──────────────────────────┐
                     │     Architect Agent      │
                     │  Expands Details/Algos   │
                     └────────────┬─────────────┘
                                  │
                                  ▼
                     ┌──────────────────────────┐
                     │  Iterative Human Review  │
                     │  Sign Off (spec accept)  │
                     └────────────┬─────────────┘
                                  │ (Autonomous Hand-off)
                                  ▼
                     ┌──────────────────────────┐
                     │   Task Execution Loop    │
                     │   Run to Completion      │
                     └────────────┬─────────────┘
                                  │
                                  ▼
                     ┌──────────────────────────┐
                     │   Compliance Feedback    │
                     │   (Success or Drift?)    │
                     └────────────┬─────────────┘
                                  ├────────────────────────┐
                   (No Drift: Done)                        │ (Drift Detected)
                                  ▼                        ▼
                     ┌──────────────────────────┐  ┌────────────────┐
                     │   Component Completed    │  │ Human Decision │
                     └──────────────────────────┘  └───────┬────────┘
                                                           │
                                   ┌───────────────────────┴───────────────┐
                                   ▼                                       ▼
                       [ Re-Prompt Redesign ]                    [ Accept Drift Changes ]
                     (Re-run Implementation)                     (spec accept updates spec)
```

### Stage 1: The Iterative Spec Design Loop

The human user does not need to write a massive, verbose specification by hand.

1. **Outline:** The human outlines the main public interfaces (Layer 1) and non-negotiable boundaries (Layer 2) inside `SPEC.md`.
2. **Expansion:** The Architect Agent builds upon that outline, fleshing out Layer 3 algorithms, error propagation, and failure states.
3. **Sign-off:** The human reviews the draft, adjusts it, and once satisfied, runs:
   ```bash
   kvist spec accept <COMPONENT_DIR>
   ```
   This formally "signs off" on the specification as accepted, freezing its hash as the current component plan.

### Stage 2: Hand-off and Autonomous Component Execution

Once the spec is completed and signed off, the human can step back and let the agent execute the component flow until completion:

- **The Microsurveillance Path:** The user can follow along task-by-task, manually invoking `kvist task run` for each TODO item, inspecting log outputs with `kvist task log`, and verifying each transition.
- **The Trust-the-Process Path:** The user lets Kvist drive the task loop autonomously, running all pending items to completion.
- **The Bubble-up Feedback Gate:** At the end of the run, the validation results bubble up. If the triple-blind review detects **specification drift** (e.g., the code implemented a behavior not described in the specification), Kvist halts and prompts the user:
  - **Option A (Trigger Redesign):** Re-prompt the developer agent with the discrepancy logs, returning the task to the execution phase.
  - **Option B (Accept Changes):** Accept the implementation changes, updating `SPEC.md` using `kvist spec accept` to incorporate the new behavior and returning the workflow back to the design phase.

---

## 4. Mapping External Agent Roles & Settings

Kvist relies on four distinct external agent roles, allowing you to configure different AI models and reasoning depths per role in your global `~/.config/kvist/config.toml`:

```toml
# Kvist Global User Configuration

[agent.profiles]
# 1. ARCHITECT: High-level reasoning for design and decomposition
[agent.profiles.architect]
command_template = "claude --non-interactive --model claude-3-5-sonnet --message '{prompt}' {context_files}"
token_limit = 100000

# 2. DEVELOPER: Highly focused, cost-effective model for TDD tests and implementation
[agent.profiles.developer]
command_template = "gemini-cli --model gemini-2.5-flash --prompt '{prompt}' --files {context_files}"
token_limit = 50000

# 3. AUDITOR: Specialized security auditing agent
[agent.profiles.auditor]
command_template = "claude --non-interactive --model claude-3-5-sonnet --message '{prompt}' {context_files}"
token_limit = 50000

# 4. REVIEWER: Neutral, source-blind clean-slate documenter and compliance checker
[agent.profiles.reviewer]
command_template = "gemini-cli --model gemini-2.5-pro --prompt '{prompt}' --files {context_files}"
token_limit = 100000
```

### Role Taxonomy:

- **Architect (Stage 1 & 2):** System decomposition, specification interview, and TODO task generation. (Highly suited to premium reasoning models like Claude 3.5 Sonnet / Gemini 2.5 Pro).
- **Developer (Stage 3):** Writing unit tests and fulfilling code implementations. (Best suited to fast, cost-efficient models like Gemini 2.5 Flash / Claude Haiku).
- **Auditor (Stage 4):** Evaluates boundaries, safety invariants, and validates memory/concurrency safety. (Requires precise, security-focused models).
- **Reviewer / Documenter (Stage 4):** Conducts clean-slate documentation (reverse-engineering `DOCS.md` from raw code) and performs the final spec-to-doc compliance comparison. (Requires a neutral, highly logical model).

---

## 5. Summary: A Glimpse into the Future of Engineering

Kvist elevates AI development from a series of speculative trials to a precise, auditable manufacturing line. By wrapping standard CLI tools, enforcing Test-Driven Development (TDD), and putting the developer in absolute, structured control, Kvist delivers reliability you can trust.

Whether you are building a small utility or managing a complex distributed application, Kvist ensures that your specifications remain the absolute source of truth, and your code behaves exactly as specified.
