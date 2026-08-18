# Kvist: The Spec-Driven AI Orchestrator
## Comprehensive User Guide and Development Walk-through

Welcome to **Kvist**. This guide serves as both an architectural reference and a complete, hands-on walk-through showing how Kvist can be used to construct a complex software project from scratch using user-provided AI agents.

Kvist is built on a core thesis: **AI code generation is fast but fundamentally fragile.** Without rigorous structure, letting an LLM write code directly inside a repository leads to "vibe coding"—a state of creeping technical debt, specification drift, and cascading test failures. 

Kvist solves this by enforcing a strict, double-blind development lifecycle where **human engineers act as Principal Architects** and **AI agents execute highly-scoped, atomically validated task queues.**

---

## 1. The Core Philosophy: Why Kvist?

Traditional AI tools (copilots and auto-writers) operate on a "generate and debug" loop. This model breaks down as projects scale. Kvist introduces three rigid constraints to restore software engineering rigor:

1. **Specs Over Code:** You cannot write a line of code until you have a validated specification (`SPEC.md`) and an ordered, dependency-mapped task queue (`TODOS.yaml`).
2. **Lifecycle Order Enforcement:** Implementations *cannot* precede tests. Reviewers *cannot* self-certify. A task's lifecycle is hardcoded to force quality:
   `Test Writing` $\rightarrow$ `Code Implementation` $\rightarrow$ `Security Audit` $\rightarrow$ `Compliance Review`
3. **Double-Blind Verification:** The developer agent implementing the code never writes the documentation. A clean-slate "Documenter" agent reverse-engineers a `DOCS.md` from the raw code, and a third "Reviewer" agent compares `DOCS.md` against the original `SPEC.md` to flag hallucinations or missed requirements.

---

## 2. High-Level Project Decomposition

To build a project with Kvist, you begin by breaking down your high-level idea into a modular hierarchy of components.

```
                  ┌──────────────────────┐
                  │   Root Component     │
                  │   (ROOT_CONTRACT)    │
                  └──────────┬───────────┘
                             │
              ┌──────────────┴──────────────┐
              ▼                             ▼
     ┌─────────────────┐           ┌─────────────────┐
     │  Component A    │           │  Component B    │
     │ (network/http)  │           │   (db/storage)  │
     └─────────────────┘           └─────────────────┘
```

### Stage 1: Initializing the Project Root
Run `kvist init` to set up the project. This command writes the fundamental root artifacts:
* `kvist.toml`: Project-wide settings and directory discovery limits.
* `ROOT_CONTRACT.md`: The immutable architectural rules, change-management protocols, and compliance standards for the entire codebase.

### Stage 2: Slicing into Modular Components
In Kvist, a **component is simply a directory with a specification (`SPEC.md`) and a queue (`TODOS.yaml`)**. 
* Organize your folders to match your logical component tree (e.g., `src/parser`, `src/storage`).
* Create a new component using `kvist spec new <COMPONENT_DIR>`. This command populates a deterministic, layered `SPEC.md` template consisting of three progressive-disclosure sections:
  * **Layer 1: Contract & Public Interface** (What the caller depends on).
  * **Layer 2: Internal Invariants & Boundaries** (Memory, safety, and security limits).
  * **Layer 3: Algorithms & Failure Paths** (Concrete implementation details).

### Stage 3: Planning the Task Queue
Using your configured **Architect Agent Profile** (which runs a high-reasoning model), you break the `SPEC.md` down into atomic work items inside `TODOS.yaml`. 
* Each task maps to a specific heading or requirement locator in `SPEC.md` (e.g., `SPEC.md#Layer-2-Boundaries`).
* Tasks must declare their depends-on relationships, forming a directed acyclic graph (DAG) where test tasks are parents to implementation tasks.

---

## 3. The Component Development Flow: A Walk-through

Let’s trace the development of a concrete project idea: **An HTTP Request Parser**. We want to implement this under `src/http_parser`.

```
                    [ HUMAN ARCHITECT ]
                     Drafts SPEC.md
                           │
                           ▼
                  [ ARCHITECT AGENT ]
                  Generates TODOS.yaml
                           │
                           ▼
                 [ DEVELOPER AGENT ]
            Fulfills `task run` for tests
                           │
                           ▼
                 [ DEVELOPER AGENT ]
            Fulfills `task run` for code
                           │
                           ▼
                  [ AUDITOR AGENT ]
               Conducts Security Audit
                           │
                           ▼
                  [ REVIEWER AGENT ]
           Executes Compliance Verification
```

### Step 1: Initialize the Component
The Human Architect initiates the parser component:
```bash
kvist spec new src/http_parser
```
This writes a template `SPEC.md` at `src/http_parser/SPEC.md`.

### Step 2: Define the Interface and Boundaries
The Human Architect fills out the layered specification:
* **Layer 1:** Define the `parse_request(raw: &[u8]) -> Result<Request, ParseError>` public signature.
* **Layer 2:** Establish bounds—reject headers longer than 8 KiB; prevent buffer overflows; restrict request methods to GET, POST, and PUT.
* **Layer 3:** Implement a state-machine algorithm that parses line-by-line using CRLF (`\r\n`) markers.

### Step 3: Schedule the Task Queue
The Architect AI model reads the completed `SPEC.md` and generates the `TODOS.yaml` containing the ordered dependencies:
1. `write-parser-tests` (Kind: `Test`): Explicitly writes unit tests for GET/POST methods and boundary violations (>8 KiB).
2. `impl-parser-logic` (Kind: `Implementation`, depends on `write-parser-tests`): Fulfills the parsing logic so all unit tests pass.
3. `parser-security-audit` (Kind: `SecurityAudit`, depends on `impl-parser-logic`): Reviews bounds and memory usage.
4. `parser-compliance-review` (Kind: `ComplianceReview`, depends on `parser-security-audit`): Runs the triple-blind compliance check.

The developer tracks this queue using:
```bash
kvist status src/http_parser
```

### Step 4: Write the Tests (Test-Driven Development)
The human hands off task execution to Kvist's automated loop. Since `write-parser-tests` has no incomplete dependencies, it is ready:
```bash
kvist task run src/http_parser write-parser-tests
```
Kvist slices the context (providing only the `src/http_parser` subdirectory, its `SPEC.md`, its `TODOS.yaml`, and the global `ROOT_CONTRACT.md`), launches your configured local agent runner (using your cheaper/faster **Developer Profile**), and redirects logs silently.

The agent writes the tests to `src/http_parser/tests.rs` (initially failing because the code is empty) and exits.

To review what the agent did or see raw compiler outputs, the human operator runs:
```bash
kvist task log src/http_parser write-parser-tests
```

### Step 5: Fulfill the Code Implementation
With the tests task successfully completed, `impl-parser-logic` becomes the next eligible task. The human fires:
```bash
kvist task run src/http_parser
```
*(Omiting the task ID automatically selects the next eligible task: `impl-parser-logic`)*.

The agent runner modifies `src/http_parser/lib.rs`, implements the state machine, and verifies that the tests now pass successfully.

### Step 6: Security Audit & Triple-Blind Review
* **Security Audit:** An independent Auditor agent reviews the implementation against the Layer 2 boundaries and certifies that no memory safety hazards or out-of-bounds reads are possible.
* **Compliance Review:** 
  1. A clean-slate agent reverse-engineers `src/http_parser/DOCS.md` strictly from the implemented code, knowing nothing of the original spec.
  2. A reviewer agent compares `DOCS.md` against `SPEC.md`. If there is 100% compliance, the task queue is marked complete.
  3. If there is a mismatch (e.g. the developer agent implemented a DELETE method not in the spec), a conflict is flagged, prompting the human architect to arbitrate.

---

## 4. Current State: Settled Architecture vs. Upcoming Roadmap

Kvist is under active development. Below is a detailed outline of our current production-grade capabilities versus what is upcoming on our roadmap.

### ───────── Settled Architecture (Fully Implemented & Verified) ─────────

* **Config-Precedence Chain:** Config is resolved dynamically: Local Project Override (`.kvist/config.toml`) $\rightarrow$ Root `kvist.toml` `[agent]` $\rightarrow$ XDG User Home (`~/.config/kvist/config.toml`) $\rightarrow$ System Global (`/etc/kvist/config.toml`).
* **Safe CLI Interpolation templates:** Commands are split POSIX-style and executed as direct OS sub-commands without shell wrapper risk, preventing shell injection. Placeholders `{prompt}`, `{context_files}`, and `{target_directory}` are fully supported.
* **Asynchronous Progress Logs:** External agent stdout/stderr is written in real-time to log files (`.kvist/logs/<task>_<timestamp>.log`). Real-time terminal streaming is toggled via `--stream`.
* **The `kvist task log` command:** Operator utility to easily inspect and output raw execution logs.
* **The `kvist spec accept` command:** Revalidates component specifications, updates TOD0.yaml hashes, and clears staleness programmatically.
* **VCS Durability Enforcement:** All Kvist core assets must be tracked in Git or Jujutsu before tasks can be transitioned or run, preventing silent, uncommitted loss.

### ───────── Upcoming Features & Roadmap (In Progress / Placeholders) ─────────

```
[ PLACEHOLDER: P2-05b ─ Test Executions Sandboxing ]
Status: IN DESIGN
Target: Safely executes repository test commands in isolated sandboxes 
(e.g., Docker, WASM, or gVisor) to protect the host machine against 
malicious, hallucinated, or unvalidated AI-generated script modifications.
```

```
[ PLACEHOLDER: UX-05 ─ Multi-Platform Shell Completions ]
Status: PLANNED
Target: Ship a completions sub-command utilizing clap_complete to generate 
highly responsive tab-completion bindings natively on Bash, Zsh, Fish, 
and PowerShell.
```

```
[ PLACEHOLDER: UX-07 ─ Uniform Structured JSON Outputs ]
Status: PLANNED
Target: Introduce a global `--json` flag on all subcommands. This returns 
stable, documented schemas on stdout, enabling third-party IDE extensions 
or custom dashboards to wrap Kvist programmatically.
```

```
[ PLACEHOLDER: Phase 3 ─ AI Skill Prompt Books ]
Status: IN DESIGN
Target: Expose specialized prompting catalogs ("skills") for writing 
unit tests, clean-slate reverse-engineering, security audits, and 
specification feasibility analysis, ensuring predictable, high-quality results.
```

---

## Summary: A Glimpse into the Future of Engineering

Kvist elevates AI development from a series of speculative trials to a precise, auditable manufacturing line. By wrapping standard CLI tools and putting the developer in absolute, structured control, Kvist delivers reliability you can trust. 

Whether you are building a small library or managing a complex distributed system, Kvist ensures that your specifications remain the absolute source of truth, and your code behaves exactly as specified.
