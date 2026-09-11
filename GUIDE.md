# Kvist Workflow & User Guide

Kvist is a local, filesystem-native execution harness and workflow engine for **human-directed, architecture-driven software engineering**. It wraps autonomous coding agents in strict, deterministic state machines, ensuring unmonitored agent execution is bounded, auditable, and mathematically contained.

The core architecture flows from top-level vision down to concrete verified code:
```
VISION.md  ──►  ARCHITECTURE.md  ──►  ROOT_CONTRACT.md
                                              │
                      ┌───────────────────────┴───────────────────────┐
                      ▼                                               ▼
          Component: src/network                          Component: src/storage
          ├── REQUIREMENTS.md                             ├── REQUIREMENTS.md
          ├── CONTRACT.md                                 ├── CONTRACT.md
          ├── DESIGN.md                                   ├── DESIGN.md
          ├── TODOS.yaml                                  ├── TODOS.yaml
          └── IMPL.md                                     └── IMPL.md
```

Executable releases currently support Linux (`x86_64`). macOS and Windows support remain deferred until native test environments and independently reviewed namespace execution backends are available.

---

## 1. Quick Start

### 1.1 Installation

Kvist is built from source using standard Cargo:

```bash
cargo build --release -p kvist -p kvist-sandbox-runner -p agent-runtime
```

Install the binaries into your `$PATH`:
```bash
cargo install --path engine
cargo install --path sandbox_runner
```

Verify the installation:
```bash
kvist --version
kvist --help
```

### 1.2 The Interactive Shell

For the best developer experience, launch Kvist's interactive shell:

```bash
kvist shell
```

The shell provides:
* **Contextual Auto-Completion:** Press `<TAB>` to autocomplete commands, flags, subcommands, component directory paths, active task IDs, attempt IDs, and configured model names.
* **Live Status Prompt:** Displays active VCS branch, current component focus, and active task lock state.
* **Built-in Terminal Pager & Spinner:** View execution logs and streaming agent output without leaving the session.
* **Audit Journaling:** Persists an append-only session history for reproducible workflows.

---

## 2. Three Paths to Getting Started

Depending on your codebase status, Kvist provides three distinct entry paths:

| Path | Command | Best Used For |
| :--- | :--- | :--- |
| **Path A: Start Fresh** | `kvist init <DIR>` | Creating a brand-new project with greenfield architecture. |
| **Path B: Convert Project** | `kvist convert <DIR>` | Bringing an existing Rust crate into Kvist without altering source files. |
| **Path C: Reverse Discovery** | `kvist reverse-discover <DIR>` | Analyzing an existing legacy codebase (Rust, Python, polyglot) and generating draft specs. |

---

### Path A: Starting Fresh from Scratch

Use this path when starting a new project or greenfield service.

#### Step 1: Initialize the Project
```bash
kvist init my-project
cd my-project
```

`kvist init` scaffolds the root architectural artifacts:
* `VISION.md`: High-level business and product goals.
* `ARCHITECTURE.md`: High-level system architecture and component boundaries.
* `ROOT_CONTRACT.md`: Non-negotiable global architectural rules, security policies, and invariants.
* `deny.toml` & `Cargo.toml`: Safe dependency limits and workspace declarations.
* Root component files (`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`).

#### Step 2: Validate System Health
```bash
kvist doctor .
kvist status .
```
`kvist doctor` verifies file permissions, VCS status (Git/Jujutsu), and artifact schema versions without modifying any files.

#### Step 3: Configure Agent Profiles
Configure which LLM or agent backend drives your tasks:
```bash
kvist agent setup
```
This launches an interactive wizard that detects local models (`llama-server`, `Ollama`) or installed CLI tools (`gemini`, `copilot`), tests them with a live qualification prompt, and registers their profile.

#### Step 4: Define Child Components
As your architecture expands, carve out isolated child components:
```bash
kvist component new src/storage
kvist component validate src/storage
```
Edit the generated `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` in `src/storage/` to define the component's boundaries.

#### Step 5: Accept Intent
Once intent documents are authored and reviewed, record their cryptographic revisions into `TODOS.yaml`:
```bash
kvist component accept src/storage
```

#### Step 6: Execute and Advance Tasks
Advance tasks through the queue:
```bash
# View the next ready task
kvist task next src/storage

# Approve test policy before execution
kvist task approve-policy .

# Run the task via the sandboxed agent
kvist task run src/storage implement-code --stream
```

---

### Path B: Converting an Existing Rust Project

Use this path when you have an existing Rust repository (`Cargo.toml` and `src/`) and want to govern it with Kvist without modifying or breaking any of your existing code.

#### Step 1: Run Conversion
From your project directory:
```bash
kvist convert .
```

#### What `kvist convert` Does:
1. **Zero-Touch Guarantee:** Leaves your `Cargo.toml`, `Cargo.lock`, and `src/**/*.rs` files completely untouched.
2. **Draft Artifact Creation:** Reads your crate manifest (name, version, dependencies, features) and synthesizes draft intent documents into a private `.kvist/` directory:
   * `.kvist/REQUIREMENTS.md`
   * `.kvist/CONTRACT.md`
   * `.kvist/DESIGN.md`
   * `.kvist/TODOS.yaml`
   * `.kvist/IMPL.md`
   * `.kvist/COMPLIANCE_REVIEW.md`
3. **Idempotence:** Safe to run repeatedly; if metadata already exists, it will not overwrite your edits.

#### Step 2: Review and Promote Drafts
Inspect the generated drafts in `.kvist/`:
```bash
cat .kvist/REQUIREMENTS.md
cat .kvist/CONTRACT.md
```
Refine the requirements and contracts to accurately document your crate's public APIs and guarantees. When satisfied, move them into their permanent locations:
```bash
mv .kvist/*.md .
mv .kvist/TODOS.yaml .
```

#### Step 3: Validate and Accept
```bash
kvist component validate .
kvist component accept .
```
Your project is now fully managed by Kvist.

---

### Path C: Reverse Discovery of an Existing Codebase

Use this path when onboarding legacy repositories, multi-language codebases (e.g. Python, TypeScript, C/C++), or projects with minimal existing documentation.

#### Step 1: Run Reverse Discovery
```bash
kvist reverse-discover /path/to/existing-codebase
```

#### What `kvist reverse-discover` Does:
1. **Recursive Source Analysis:** Scans the codebase, identifying source files, test suites, external documentation, and exposed public functions/classes.
2. **Non-Destructive Staging:** Places all reverse-engineered documents in `.kvist/` to prevent overwriting any existing project files.
3. **Draft Synthesis:** Automatically drafts:
   * **`REQUIREMENTS.md`:** Reverse-engineers functional requirements and acceptance criteria from existing test suites and documentation.
   * **`CONTRACT.md`:** Documents public API exports, public types, and invariants discovered in the code.
   * **`DESIGN.md`:** Outlines architectural modules, dependencies, and internal mechanics.
   * **`TODOS.yaml`:** Synthesizes a structured lifecycle task queue (`test` -> `implementation` -> `security-audit` -> `compliance-review`).
   * **`IMPL.md`:** Generates an initial observed implementation record.

#### Step 2: Human Audit & Specification Refinement
*Because reverse discovery infers intent from implementation, the generated contract is non-normative.*

1. Open `.kvist/REQUIREMENTS.md` and `.kvist/CONTRACT.md`.
2. Review the extracted symbols, verify error boundaries, and add any unwritten architectural invariants.
3. Once refined, copy the artifacts to the component root and run:
```bash
kvist component validate .
kvist component accept .
```

---

## 3. The 5 Core Component Artifacts

Every Kvist component is governed by five standard files:

| Artifact | Author / Source | Role & Guarantees |
| :--- | :--- | :--- |
| **`REQUIREMENTS.md`** | Architect / Stakeholder | Owns outcomes, user stories, resource constraints, and testable acceptance criteria. |
| **`CONTRACT.md`** | Architect / Component Owner | Owns external observable interfaces, public APIs, wire protocols, and invariants. |
| **`DESIGN.md`** | Tech Lead / Designer | Owns internal state machines, algorithms, internal file structure, and technical mechanics. |
| **`TODOS.yaml`** | Designer / Lead | The durable task queue. Declares tasks, dependencies, kinds (`test`, `implementation`, `security-audit`, `compliance-review`), and revision digests. |
| **`IMPL.md`** | Clean-Slate Documenter | Observed implementation evidence. Documented purely from source code without reading intent documents. |

---

## 4. Contract Inheritance & Staleness Detection

Kvist uses strict, unidirectional architectural boundaries:
1. **Nearest Parent Inheritance:** A child component inherits only its immediate parent component's `CONTRACT.md` and the global `ROOT_CONTRACT.md`. Peer components cannot see each other's internal implementation.
2. **Transparent Namespaces:** If a component resides in `src/drivers/storage/nvme`, Kvist walks up the directory tree to find the nearest actual component ancestor, even across intermediate namespace folders.
3. **Cryptographic Staleness Tracking:** Whenever a parent's `CONTRACT.md` is modified, Kvist marks all child components as **stale**:
```
$ kvist status .
component: src/storage/nvme state: stale
  cause: parent contract revision mismatch (expected sha256:abc..., found sha256:def...)
```
To clear staleness, review the parent changes and run `kvist component accept <DIR>`.

---

## 5. Sandboxed Agent Execution

Kvist never executes unconstrained coding agents directly on your host. Agent execution is secured via a multi-layer defense:

```
+--------------------------------------------------------------------------+
| HOST SYSTEM                                                              |
|                                                                          |
|   kvist engine  ──(request json)──►  kvist-sandbox-runner                |
|                                             │                            |
|                                             ▼ (Linux Namespaces)         |
|   +------------------------------------------------------------------+   |
|   | Bubblewrap Container (User, IPC, PID, UTS, Cgroup, Net)          |   |
|   |                                                                  |   |
|   |   - Writable: Target Component Directory (/workspace/component)  |   |
|   |   - Read-Only: ROOT_CONTRACT.md, PARENT_CONTRACT.md              |   |
|   |   - Blocked: All other project directories                       |   |
|   |   - Network: Mediated via Source-Aware Proxy to Registries       |   |
|   |   - Limits: prlimit (FDs, File Size, Output Bytes, Timeouts)     |   |
|   |                                                                  |   |
|   |   [ Coding Agent / Compiler / Test Suite ]                       |   |
|   +------------------------------------------------------------------+   |
+--------------------------------------------------------------------------+
```

### 5.1 Approving Test Policies
Before executing implementation tasks, the human operator must explicitly approve the test policy:
```bash
kvist task approve-policy .
```
This cryptographically signs an approval record locking the test command, runner binary identity, and resource limits. If any binary or policy is modified, Kvist refuses execution until re-approved.

### 5.2 Running a Task
```bash
# Run the next ready task
kvist task run .

# Run a specific task with real-time output streaming
kvist task run . implement-code --stream
```

### 5.3 Automated Unattended Task Loop
To advance all ready tasks in dependency order unattended:

```bash
while task_id="$(kvist task next .)" && [ "$task_id" != "no ready task" ]; do
  echo "Executing task: $task_id"
  kvist task run . "$task_id" || exit $?
done
kvist status .
```

### 5.4 Replaying Execution Trajectories
Every execution turn, tool call, and token metric is journaled into `.kvist/runs/`. You can replay historical trajectories offline:

```bash
kvist task replay .kvist/runs/implement-code_2026-09-11T22-00-00Z.trajectory.jsonl
```

---

## 6. Review & Compliance Discipline

To prevent models from hallucinating success or certifying their own code:

1. **Clean-Slate Documentation:** `IMPL.md` must be authored by an agent that reads only the source code and test outputs, with zero access to `REQUIREMENTS.md`, `CONTRACT.md`, or previous task logs.
2. **Source-Blind Compliance Review:** A separate reviewer compares the authored intent (`REQUIREMENTS.md`, `CONTRACT.md`) against `IMPL.md` and test results, **without access to the source code**.
3. **Architectural Arbitration:** The human architect arbitrates any discrepancies between intent and implementation.

For detailed instructions on compliance reviews, refer to [`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md) and [`COMPLIANCE_REVIEW.md`](COMPLIANCE_REVIEW.md).

---

## 7. Command Reference Cheat-Sheet

### Shell & Diagnostics
| Command | Description |
| :--- | :--- |
| `kvist shell` | Start the interactive development shell with auto-completions. |
| `kvist doctor [DIR]` | Check project health, versions, and VCS configuration. |
| `kvist status [DIR]` | Inspect component lifecycle status, document staleness, and locks. |
| `kvist tree [DIR]` | Render an ASCII component tree. |
| `kvist completions <SHELL>` | Generate shell auto-completion scripts (`bash`, `zsh`, `fish`, `powershell`). |

### Onboarding & Creation
| Command | Description |
| :--- | :--- |
| `kvist init [DIR]` | Initialize a greenfield project with root artifacts. |
| `kvist convert <DIR>` | Convert an existing Rust project into Kvist without editing code. |
| `kvist reverse-discover <PATH>` | Reverse-engineer draft specs and task queue from any existing codebase. |
| `kvist import <REPO_URL>` | Import Kvist components from an external Git repository. |

### Component Management
| Command | Description |
| :--- | :--- |
| `kvist component new <PATH>` | Scaffold a new child component directory with valid Markdown templates. |
| `kvist component validate <PATH>` | Structurally validate component intent documents. |
| `kvist component accept <PATH>` | Cryptographically accept intent documents and update queue revisions. |

### Task Management & Execution
| Command | Description |
| :--- | :--- |
| `kvist task next [DIR]` | Print the next ready task ID. |
| `kvist task approve-policy [DIR]`| Cryptographically approve the sandbox test-execution policy. |
| `kvist task run [DIR] [TASK]` | Execute a task inside the Bubblewrap sandbox. |
| `kvist task log <DIR> <TASK>` | Display the execution log of a completed or failed task. |
| `kvist task replay <SESSION>` | Step through a structured execution trajectory journal. |
| `kvist task unlock [DIR]` | Release stale or orphaned component execution locks. |
| `kvist task recover <DIR> <TASK>` | Safely recover an interrupted or blocked task. |
| `kvist task finalize <DIR> <TASK> <ATTEMPT>` | Review and finalize task execution evidence. |
| `kvist task transition <DIR> <TASK> <STATUS>` | Perform an audited task state transition (`pending`, `in-progress`, `blocked`, `completed`). |
