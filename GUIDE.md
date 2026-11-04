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
cargo build --release -p kvist -p galla-runner -p sav
```

Install the binaries into your `$PATH`:

```bash
cargo install --path maerg
cargo install --path galla
```

Verify the installation:

```bash
kvist --version
kvist --help
```

### 1.2 The Markdown Workspace

For the best developer experience, launch Kvist's workspace inside a project:

```bash
kvist
```

The shell is a full-screen terminal UI in the shape of the `skott` layout, not a
line-oriented REPL:

Use Esc or F2 for the workspace menu and F1 for Markdown shortcut help.
Esc dismisses completion first, otherwise returns one menu level; only the
top-level menu closes back to the command. PageUp/PageDown and the
mouse wheel browse the transcript; its scrollbar spans all history pages.
Ctrl+Home jumps to the oldest page; Ctrl+End resumes following new output.
Ctrl+C cancels the running command without rolling back effects; Ctrl+D on an
empty input sends EOF to the command or exits when idle. Ctrl+Q and the Quit
menu item exit immediately when idle, or ask before cancelling/reaping active
work. Effects are not rolled back. While a command runs, footer
input answers its stdin; answers are masked and never saved.

History lives privately in `.kvist/transcript/` at project and component scope.
Numbered Markdown pages target 1 MiB and rotate between complete top-level
blocks (maximum indivisible block: 16 MiB). Older pages are not deleted.
Each page renders independently; cross-page reference links/footnotes are not
resolved as a single whole-history document. This diagnostic history is not
compliance evidence. Model/effort settings still require normal policy review
and reapproval when configuration changes. `kvist shell PATH` remains available
for explicit project selection. Redirected bare invocation stays finite.

- **Status Heading:** Shows the active component together with its state and
  task progress, plus the active VCS branch, the sandbox backend, the default
  model, and lock counts (live vs. stale). When no component is selected the
  heading shows the whole project and commands operate on the project; when a
  component is selected it is auto-selected as the default for every command
  that expects a component, so you are never asked for it and it is pre-filled.
- **Main Transcript Area:** A bounded on-screen window with a scrollbar over a
  durable, per-component, disk-persisted record of every interaction (commands
  and their rendered Markdown output). The record is unlimited in size and
  splits into successive files when one grows too large, so you can always
  scroll back to any earlier command and result. Switching component clears the
  transcript area and restores that component's former transcript when you
  return to it.
- **Command Footer:** The same command surface as the CLI with full
  tab-completion (static from the command grammar, dynamic from the project
  state) and keyboard-shortcut help rendered the same way `skott` does it.
- **Markdown-First Output:** Every command reports a well-formed Markdown
  document, and the shell renders it with the `blad` renderer so tables,
  diagrams, math, and code blocks display with full fidelity. The stable
  `status --format text` and `--format json` report forms are unchanged for
  scripts, and the plain-text degradation contract still applies when the
  terminal cannot host the TUI.
- **Workspace Menu:** Components, role assignments, models, appearance, history,
  help and quit share one hierarchy. Pickers highlight and mark their current
  value. Type to filter component/model lists; Ctrl+P in Components pins/unpins
  the highlighted entry (at most 12 pins and 12 recent components). Back-navigation
  preserves the parent selection/filter and command draft. Component selection
  is the same focus change as `cd` and pre-fills subsequent commands.
- **Role Assignments:** Inspect developer, architect and security-reviewer models,
  provider, effective effort and configuration source. Edit project or user
  defaults per role; Default/inherit is distinct from explicit `none`.
  Changing settings never changes task-kind roles or approves execution.
- **Model Library:** Inspect configured profiles without contacting providers.
  Add/check explicitly invoke bounded host discovery/qualification; credentials
  are not displayed. Registry-backed enable/disable/removal identifies the
  exact source and refuses stale or referenced definitions. Shared sources may
  have consumers outside this project; deletion can expose a fallback profile.
  Persisted settings (including appearance and pins) require an idle command;
  inspection and transcript navigation remain available during execution.
- **Run the Whole Component:** bare `task run COMPONENT` iterates the component's tasks in
  dependency order, running each ready task, and stops when the component is
  complete, when a task becomes blocked, or when a task reaches an
  awaiting-decision point. It never auto-resolves a decision or unblocks a
  blocked task; it pauses and reports the exact state and the command that
  resolves it.
- **Contextual Auto-Completion:** Press `<TAB>` to autocomplete commands,
  flags, subcommands, component directory paths, active task IDs, attempt IDs,
  and configured model names. Dynamic values carry rich descriptions (task
  status and title, a `★ next ready` marker in run contexts, the current
  component), completions track durable project state after every command, and
  a complete `--` token switches completion from flags to positionals.
- **Modern Theming, Plain-Text Fallback:** Output is styled with a consistent
  accent palette and stays column-aligned; it degrades to plain text under
  `NO_COLOR`, `CLICOLOR=0`, `TERM=dumb`, or when captured/piped
  (`CLICOLOR_FORCE` overrides).
- **Keyboard Shortcuts:** `Ctrl+L` clears the screen and redraws the prompt,
  `↑`/`↓` pick completion candidates, and `Ctrl+D` exits.
- **Audit Journaling & History:** An append-only session journal
  (`.kvist/session.log`, inspect with `journal`) and persistent editor history
  (`.kvist/history`, inspect with `history`).
- **Workflow Builtins:** `cd [COMPONENT]` sets the current component, `tasks`
  lists task status, `run` executes ready work continuously, `last` shows recent
  agent runs, `locks [clean]` inspects or removes stale locks, and `help` lists
  everything.
- **Cancellable Execution:** `Ctrl+C` during a running command cancels the
  supervised process group and returns to the prompt, leaving durable task
  state for explicit `task finalize` or `task recover`.

Typical session:

```text
kvist (main) ❯ cd maerg          # remember the component
kvist maerg/ (main) ❯ tasks      # status of maerg's tasks
kvist maerg/ (main) ❯ task run maerg # maerg is visibly prefilled; run to approval/blocker
kvist maerg/ (main) ❯ last       # recent agent runs with log links
```

---

## 2. Three Paths to Getting Started

Depending on your codebase status, Kvist provides three distinct entry paths:

| Path                          | Command                        | Best Used For                                                                              |
| :---------------------------- | :----------------------------- | :----------------------------------------------------------------------------------------- |
| **Path A: Start Fresh**       | `kvist init <DIR>`             | Creating a brand-new project with greenfield architecture.                                 |
| **Path B: Convert Project**   | `kvist convert <DIR>`          | Bringing an existing Rust crate into Kvist without altering source files.                  |
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

- `VISION.md`: High-level business and product goals.
- `ARCHITECTURE.md`: High-level system architecture and component boundaries.
- `ROOT_CONTRACT.md`: Non-negotiable global architectural rules, security policies, and invariants.
- `deny.toml` & `Cargo.toml`: Safe dependency limits and workspace declarations.
- Root component files (`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`).

#### Step 2: Validate System Health

```bash
kvist doctor .
kvist status .
```

`kvist doctor` verifies file permissions, VCS status (Git/Jujutsu), and artifact schema versions without modifying any files.

#### Step 3: Configure Agent Profiles and Assign Roles

Kvist cleanly separates **configuring a model profile** from **assigning it to a role**:

1. **Configure and test model profiles**:

```bash
kvist agent setup
```

This wizard qualifies local models (`llama-server`, `Ollama`) or installed CLI tools (`gemini`, `copilot`), tests them with a live qualification prompt, and saves them to your model catalog without requiring immediate role assignments.

2. **Assign profiles to roles**:

```bash
# View current role assignments and available models
kvist agent role

# Assign a model profile to a specific role
kvist agent role set developer qwen
kvist agent role set architect sonnet

# Clear an assignment
kvist agent role clear developer
```

3. **Customize provider discovery (optional)**:
   In `kvist.toml` (or `~/.config/kvist/config.toml`), you can configure provider discovery endpoints:

```toml
[agent.discovery]
ollama_url = "http://127.0.0.1:11434"
llama_server_url = "http://127.0.0.1:8080"
timeout_seconds = 5
allow_host_discovery = true
```

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
   - `.kvist/REQUIREMENTS.md`
   - `.kvist/CONTRACT.md`
   - `.kvist/DESIGN.md`
   - `.kvist/TODOS.yaml`
   - `.kvist/IMPL.md`
   - `.kvist/COMPLIANCE_REVIEW.md`
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
   - **`REQUIREMENTS.md`:** Reverse-engineers functional requirements and acceptance criteria from existing test suites and documentation.
   - **`CONTRACT.md`:** Documents public API exports, public types, and invariants discovered in the code.
   - **`DESIGN.md`:** Outlines architectural modules, dependencies, and internal mechanics.
   - **`TODOS.yaml`:** Synthesizes a structured lifecycle task queue (`test` -> `implementation` -> `security-audit` -> `compliance-review`).
   - **`IMPL.md`:** Generates an initial observed implementation record.

#### Step 2: Human Audit & Specification Refinement

_Because reverse discovery infers intent from implementation, the generated contract is non-normative._

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

| Artifact              | Author / Source             | Role & Guarantees                                                                                                                                    |
| :-------------------- | :-------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`REQUIREMENTS.md`** | Architect / Stakeholder     | Owns outcomes, user stories, resource constraints, and testable acceptance criteria.                                                                 |
| **`CONTRACT.md`**     | Architect / Component Owner | Owns external observable interfaces, public APIs, wire protocols, and invariants.                                                                    |
| **`DESIGN.md`**       | Tech Lead / Designer        | Owns internal state machines, algorithms, internal file structure, and technical mechanics.                                                          |
| **`TODOS.yaml`**      | Designer / Lead             | The durable task queue. Declares tasks, dependencies, kinds (`test`, `implementation`, `security-audit`, `compliance-review`), and revision digests. |
| **`IMPL.md`**         | Clean-Slate Documenter      | Observed implementation evidence. Documented purely from source code without reading intent documents.                                               |

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
|   kvist maerg  ──(request json)──►  galla-runner                |
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
# Run all dependency-ready work, checkpointing complete items
kvist task run .

# Run a specific task with real-time output streaming
kvist task run . implement-code --stream

```

An exact `TASK` runs only that task if its prerequisites are fulfilled.
Omit it for continuous execution without confirmation. There are no batch
selectors or separate whole-component command.

### 5.3 Automated Unattended Task Loop

The simple form runs the whole component's remaining work in one command:

```bash
kvist task run .
kvist status .
```

```toml
[task_run]
commit = "item" # default; alternatives: "component" or "manual"
```

Each default checkpoint waits for every queued stage of its item, including
its tests, implementation and any reviews. Items use a shared task-ID prefix
before the lifecycle suffix and must be contiguous in the queue. Automatic
commits require an existing attached Git HEAD and clean overlapping paths;
unrelated staged work is preserved and nothing is pushed. Commits are
**unaccepted checkpoints**, not human approval.
If Git signing is enabled, prepare the signing helper for unattended use.
Git/signing commands honor cancellation and stop after five minutes instead of
hanging indefinitely; signing policy is never bypassed.
These raw-byte checkpoints disable Git hooks and clean filters; approved task
verification runs separately.

Work stops at a decision, blocker, cancellation, failed checkpoint, or final
component approval. Fix a reported signing/Git failure and repeat the same bare
run to retry the checkpoint without rerunning completed tasks. Failed or
interrupted effects retain existing explicit recovery/disposition requirements.
Changing commit policy requires `kvist task approve-policy`.
The final pending-approval record and independent review reports are included
in the last checkpoint. `component accept` still accepts intent only.
There is not yet a dedicated component implementation-approval command.
The component argument is required; shell focus inserts it visibly instead of
silently supplying it when omitted. Independent reviews retain task logs and
live diagnostics; their opt-in controls permit cancellation but reject guidance
to preserve clean-slate review boundaries.
After manual edits during a pause, inspect verification and review freshness:
completed reviews are not automatically invalidated or rerun.

### Decisions and blockers

When work needs your attention, Kvist prints a specific `.md` brief under the
component's `.kvist/attention/`. Open it manually until transcript rendering is
fully integrated. Agent explanations describe the situation, design impact,
alternatives and tradeoffs, recommendation and uncertainty. Host-written
instructions explain how to discuss, stage a complete decision change-set, and
apply exact approval without editing queue YAML.

Review JSON remains available with a same-stem Markdown companion under
`.kvist/continuous-reviews/`; `component.md` indexes final review evidence.
Operational briefs are outside automatic checkpoint ownership so a failed
checkpoint can be explained without making its retry dirty. Immutable,
content-addressed brief history remains beside each latest task/run alias.
If inference fails, Kvist writes an explicitly labelled diagnostic rather than
calling the failing model again. No summary chooses an approach or accepts work.
Use the required visible component argument (the shell prefills it):

```bash
kvist decision galla
kvist decision galla discuss "What is established, and what needs investigation?"
kvist decision galla options
# Choose the displayed option response number (example only):
kvist decision galla do 3
# Read the generated plan, then choose its "approve and continue" response:
kvist decision galla do 5
```

Numbers above are examples: use the responses actually displayed for your
component. `decision galla actions` lists them again and also indexes plans
prepared before numbered support existed. Tab after `decision galla do `
offers numbers with descriptions and full command expansions. A number stays
bound to its original command; changed catalogs or stale plans are refused,
never redirected to the latest choice. Apply-only and apply-and-continue have
separate numbers. Invocation is explicit choice/approval, not an extra prompt.
No model Markdown is executed as shell code. Hashes remain in canonical
evidence and the advanced `apply PLAN_ID --approve SHA256` interface.
Expanded option commands include `--catalog SHA256` to pin their exact catalog;
running that full command and its response number enforces the same freshness.
For added guidance, `plan --option investigate --guidance "Preserve all current guarantees"`
still stages a plan and returns short approval responses.

Stable option IDs come from the generated catalog, not Markdown paragraph positions;
there may be 1-32 substantive choices. With multiple pauses, specify
`--task TASK_ID` (`show TASK_ID` for inspection). Follow-up discussion is
durable and advisory. Choosing stages a proposal; it does not change controlled
files. The planner returns typed intent/task effects, including prerequisites
before a resumed audit. A separate bounded intent review is recorded, or you
must supply an explicit `--review-exception REASON`.

The Markdown preview shows all effects and one content-bound approval command.
Apply invokes no model, accepts only those exact intent/scheduling changes and
does not approve implementation, certify findings, commit or push. `--continue`
uses the existing independently approved execution policy. Changed inputs
require a new plan; an interrupted application fences ordinary writers until
the identical approved command finishes without overwriting external edits.

Reject/redirect with `plan --guidance "Reject those suggestions; instead ..."`.
To discard work, explicitly request withdrawal with reasons and the entire
affected uncompleted dependency closure; records remain durable and are never
marked completed. Completed/live/fenced tasks cannot be withdrawn. Existing
source removal requires separately scheduled implementation.

Manual/offline plans use the same strict native JSON version-1 draft format as
agent plans, documented in [maerg/CONTRACT.md](maerg/CONTRACT.md):

```json
{"schema_version":1,"rationale":"The environment is repaired; retry with guarantees unchanged.","documents":{},"tasks":[],"disposition":"resolve"}
```

```bash
kvist decision galla plan --import draft.json --review-exception "I reviewed this unchanged-intent retry"
```

Then inspect and apply the printed exact approval command. Raw queues, statuses,
timestamps and acceptance IDs are not draft fields. `repair` remains artifact
recovery, not a means to bypass reviewed product changes. `.kvist/decisions/`
retains Markdown/JSON options, discussion, immutable plans and application
receipts and the authenticated numbered-response registry. Do not delete that
registry to renumber responses; retain historical numbers. Historical attention
briefs and audit reports are not rewritten.
Historical and stale responses remain listed; freshness is checked when used.
At the bounded registry limit (2,048 actions or 1 MiB), use the reported full
command rather than deleting state to recycle numbers.
For large intent documents, prefer `document_edits` with exact unique
`path`/`before`/`after` excerpts instead of regenerating full files. The host
materializes and validates complete files for the same exact approval preview;
ambiguous or missing anchors fail rather than guessing.

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

For detailed instructions on compliance reviews, refer to [`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md).

---

## 7. Command Reference Cheat-Sheet

### Shell & Diagnostics

| Command                     | Description                                                                                                                                                                                  |
| :-------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `kvist` _(no args)_         | Inside a project: the project overview and what to do next. Outside: the guided help.                                                                                                        |
| `kvist`                     | Open the Markdown workspace with command input and dynamic tab-completion. |
| `kvist doctor [DIR]`        | Check project health, versions, and VCS configuration.                                                                                                                                       |
| `kvist status [DIR]`        | "Where do I stand?" States, task progress, changed documents with expected/observed revisions, and the exact next action per component. `--format text\|json` for stable script forms.       |
| `kvist tree [DIR]`          | Render an ASCII component tree.                                                                                                                                                              |
| `kvist help [TOPIC]`        | Guided help: the 30-second tour, or a topic — `concepts`, `lifecycle`, `task-states` (states, legal transitions, and the command that achieves each). Tab-completable; also a shell builtin. |
| `kvist completions <SHELL>` | Generate shell auto-completion scripts (`bash`, `zsh`, `fish`, `powershell`).                                                                                                                |

Every command resolves the project by walking upward from the current
directory to the nearest `kvist.toml`, and component commands act on the
component containing the current directory — so `cd maerg && kvist task next`
works without repeating the component. An explicit path argument always wins.

### In-Shell Builtins

Type these directly at the `kvist` prompt (no `kvist` prefix needed):

| Command                          | Description                                                                                                                         |
| :------------------------------- | :---------------------------------------------------------------------------------------------------------------------------------- |
| `cd [COMPONENT]`                 | Set the current component: `run`, `tasks`, completion, and dispatched commands like `task next` / `component accept` all target it. |
| `tasks [COMPONENT] [--status S]` | List tasks with status; filter by `pending`, `in-progress`, `blocked`, `awaiting-decision`, `completed`.                            |
| `run COMPONENT [TASK]`         | Run an exact task, or continuous component work when TASK is omitted. |
| `last [COUNT]`                   | Show recent agent runs with tokens and log links.                                                                                   |
| `history [COUNT]`                | Show recent editor history lines.                                                                                                   |
| `journal`                        | Show the append-only session journal.                                                                                               |
| `locks [clean]`                  | Inspect live/stale task locks; `clean` removes stale ones.                                                                          |
| `help [TOPIC]`                   | List builtins and workflow commands, or show a help topic: `concepts`, `lifecycle`, `task-states` (same content as `kvist help`).   |
| `exit` / `quit`                  | Leave the shell.                                                                                                                    |

Key bindings: `<TAB>`/`<S-TAB>` complete and cycle, arrows navigate the menu,
`<ESC>` closes it, `<CTRL+C>` cancels a running command, `<CTRL+L>` clears the
screen, `<CTRL+D>` exits at an empty prompt.

### Onboarding & Creation

| Command                         | Description                                                             |
| :------------------------------ | :---------------------------------------------------------------------- |
| `kvist init [DIR]`              | Initialize a greenfield project with root artifacts.                    |
| `kvist convert <DIR>`           | Convert an existing Rust project into Kvist without editing code.       |
| `kvist reverse-discover <PATH>` | Reverse-engineer draft specs and task queue from any existing codebase. |
| `kvist import <REPO_URL>`       | Import Kvist components from an external Git repository.                |

### Component Management

`<PATH>` is optional in `validate` and `accept`: it defaults to the component
containing the current directory.

| Command                           | Description                                                             |
| :-------------------------------- | :---------------------------------------------------------------------- |
| `kvist component new <PATH>`      | Scaffold a new child component directory with valid Markdown templates. |
| `kvist component validate [PATH]` | Structurally validate component intent documents.                       |
| `kvist component accept [PATH]`   | Cryptographically accept intent documents and update queue revisions.   |
| `kvist component commit <ID>`     | (Re)create the isolated acceptance commit for a pending acceptance.     |

### Task Management & Execution

`<DIR>` is optional everywhere: it defaults to the component containing the
current directory.

| Command                                                    | Description                                                                                                                                                               |
| :--------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `kvist task next [DIR]`                                    | Print the next ready task ID.                                                                                                                                             |
| `kvist task approve-policy [DIR]`                          | Cryptographically approve the sandbox test-execution policy.                                                                                                              |
| `kvist task run DIR [TASK]`                              | Run an exact task, or omit TASK for continuous dependency-ready work and complete-item checkpoint commits; stop for approval, decisions or blockers. |
| `kvist task log <DIR> <TASK>`                              | Display the execution log of a completed or failed task.                                                                                                                  |
| `kvist task replay <SESSION>`                              | Step through a structured execution trajectory journal.                                                                                                                   |
| `kvist task unlock [DIR]`                                  | Release stale or orphaned component execution locks.                                                                                                                      |
| `kvist task recover <DIR> <TASK> <ATTEMPT>`                | Reconcile a fenced attempt that verifiably never started (`--disposition execution-did-not-start`).                                                                       |
| `kvist task finalize <DIR> <TASK> <ATTEMPT> accept\|block` | Record the human disposition of a completed attempt.                                                                                                                      |
| `kvist task transition <DIR> <TASK> <STATUS>`              | Perform an audited task state transition (`pending`, `in-progress`, `blocked`, `awaiting-decision`, `completed`). See `kvist help task-states` for the legal transitions. |
