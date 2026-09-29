# Kvist Command-Set Design

This document is the authoritative design for the Kvist command-line surface.
It explains the core ideas behind the commands, how they fit together into one
workflow, and the rules that keep the interface simple, predictable, and
self-explanatory. Implementation in `engine/` MUST follow this document.

## 1. Goal

A user who knows a handful of core concepts should be able to operate Kvist
without memorizing a large command list:

- **Simple:** few commands, each with an obvious job.
- **Intuitive:** commands work from where the user is standing (`cd` in the
  real shell means the same thing to Kvist as it means to the user).
- **Helpful:** every answer — success, failure, or status — tells the user
  what is true and what to do next.
- **Powerful:** the full explicit surface remains available for scripts and
  automation, including JSON output and explicit paths.
- **Discoverable:** help is a guided tour, not a raw list; `kvist` alone
  shows the user where they are and what to do.

## 2. Core concepts

Kvist is built on exactly four concepts. The command set is organized around
them, and the help text teaches them.

1. **Project.** A directory containing `kvist.toml` plus the root artifacts
   (`VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`). The project root is
   found by walking upward from the current directory, like `git`.
2. **Component.** A directory that owns its five artifacts:
   `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`.
   A component's directory is writable; everything else in the project is
   read-only for its supervised execution.
3. **Intent documents.** `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`. They
   describe what a component MUST do before code is allowed to change.
   `kvist component accept` records their cryptographic revisions into
   `TODOS.yaml`, making them the accepted baseline.
4. **Tasks.** Entries in `TODOS.yaml` with a durable status
   (`pending`, `in-progress`, `blocked`, `awaiting-decision`, `completed`).
   `kvist task run` executes one ready task with a supervised agent.

The lifecycle every component walks:

```
author intent ──► validate ──► accept ──► run tasks ──► finalize
     ▲                                            │
     └────────── change intent (becomes stale) ◄──┘
```

When intent documents change after acceptance, the component becomes
**stale**: Kvist records *which* document changed and what the expected and
observed revisions are, and every surface (status, errors) tells the user to
review the change and run `kvist component accept` again.

## 3. Design principles

### P1 — One verb per lifecycle stage

There is one canonical command for each stage of the lifecycle and for each
cross-cutting concern. No aliases, no overlapping subgroups:

| Concern            | Canonical command                        |
| ------------------ | ---------------------------------------- |
| interactive work   | `kvist shell`                            |
| inspect state      | `kvist status` (default view), `tree`, `doctor` |
| create component   | `kvist component new`                    |
| check intent docs  | `kvist component validate`               |
| accept intent      | `kvist component accept`                 |
| pick a task        | `kvist task next`                        |
| execute a task     | `kvist task run`                         |
| inspect execution  | `kvist task log` / `task replay`         |
| human disposition  | `kvist task finalize` / `task recover`   |
| manual state       | `kvist task transition`                  |
| policy gate        | `kvist task approve-policy`              |
| lock maintenance   | `kvist task unlock`                      |
| agent configuration| `kvist agent setup|list|remove|check|role` |
| onboarding         | `kvist init` / `convert` / `reverse-discover` / `import` |
| offline build      | `kvist vendor` / `kvist toolchain`       |

Removed from the previous surface (and why):

- `kvist overview` — merged into `kvist status`. `status` renders the
  human-friendly overview by default; `--format text` and `--format json`
  keep the stable report forms for scripts. One command answers
  "where do I stand?".
- `kvist agent profile add|list|remove` and the `kvist agent setup|list|remove`
  alias pair — collapsed to the single `kvist agent setup|list|remove`
  commands. Two names for the same action was noise.
- `kvist toolchain ensure` — the only operation; the subcommand level was
  ceremony. `kvist toolchain [PROJECT_DIR]` does it.
- `kvist vcs commit-accepted` — the acceptance-commit concept belongs to the
  component that owns acceptance. It is now `kvist component commit
  <ACCEPTANCE_ID>`.

### P2 — The working directory is context

`cd` in the user's shell is meaningful to every Kvist command:

- **Project resolution.** Project-level commands (`status`, `tree`,
  `doctor`, `shell`, `vendor`, `toolchain`, `task approve-policy`) resolve
  the project root by walking upward from the current directory to the
  nearest `kvist.toml`. An explicit `PROJECT_DIR` argument always wins and
  is interpreted relative to the current directory, exactly as before.
- **Component resolution.** Component-level commands (`task ...`,
  `component validate|accept`) take an OPTIONAL `COMPONENT_DIR`. When it is
  omitted, Kvist selects the nearest Kvist component that contains the
  current directory (the root component `.` when standing at the project
  root). When it is given, it is interpreted relative to the project's
  component root, as before.
- **The shell's `cd` builtin** does the same job interactively: after
  `cd engine`, every dispatched command — `task next`, `component
  validate`, `task run`, ... — targets `engine` without repetition. The
  prompt shows the focus, and typing a command that names a *different*
  component prints a one-line reminder.
- **Failure outside a project** is never cryptic: the error names what is
  missing and suggests the two exits (`kvist init` here, or pass an
  explicit directory).

Scripts and one-shot uses pass explicit paths; interactive users do not
repeat themselves. Both forms are first-class.

### P3 — Every answer contains the next step

Guidance is part of the contract, not an afterthought:

- **Errors** are two-part: a precise statement of the problem, then a
  `hint:` line with the concrete command that unblocks the user. Examples:
  a stale component being run → "run `kvist component accept <dir>` after
  reviewing the changed documents"; an unapproved policy → "run
  `kvist task approve-policy`"; no ready task → "inspect
  `kvist task next` / `status`".
- **Status** renders, per component: its state, which artifacts are valid,
  task progress, the next ready task, and — for every non-`current` state —
  an **Action** line with the exact command to run. For stale components it
  lists each changed document with expected vs. observed revision and a
  hint to diff the listed documents against the last accepted revision
  (e.g. `git diff HEAD -- <path>` under Git) to see *what* changed.
- **Bare `kvist`** (no arguments) inside a project prints the same overview
  as `kvist status` plus a footer pointing at `kvist help` — so the answer
  to "what do I do now?" is always one keystroke away. Outside a project it
  prints the guided help.
- **Success messages** for gating commands name the next stage: after
  `component accept`, the output says the component is current and points
  at `task next`; after a run finishes, it points at `task log` and
  `task finalize`.

### P4 — Help is a tour

- Top-level `kvist --help` leads with the typical flow (five commands)
  before listing the surface; every command's `about` text says *when to
  use it*, not just what it does.
- The shell's `help` builtin lists builtins, then the workflow commands
  that matter day-to-day, with their focus-aware forms (no component
  argument needed after `cd`).
- `kvist completions <SHELL>` remains for tab completion outside the shell.

### P5 — Explicit stays deterministic

Explicit arguments are always accepted and win over context resolution.
`--json` remains global and stable. The stable text report
(`status --format text`) keeps its `status-format-version`. JSON outputs
keep their documented shapes. Nothing interactive is ever required of a
script: no confirmation, no pager, no terminal.

## 4. The command surface

### 4.1 Top level

| Command | Form | Purpose (when to use) |
| ------- | ---- | --------------------- |
| `kvist` | *(no args)* | Inside a project: overview + what to do next. Outside: guided help. |
| `kvist shell [PROJECT_DIR]` | start the interactive workspace shell (recommended entry point) |
| `kvist status [PROJECT_DIR] [--format overview\|text\|json] [--only-documents] [--only-impls] [--unfinished]` | "where do I stand?" — overview by default; stable text/json for scripts |
| `kvist tree [PROJECT_DIR]` | browse the component hierarchy |
| `kvist doctor [PROJECT_DIR]` | verify root artifacts, VCS, versions (read-only health check) |
| `kvist init [PROJECT_DIR]` | start a new project here |
| `kvist convert PROJECT_DIR` | onboard an existing Rust crate (drafts in `.kvist/`) |
| `kvist reverse-discover PATH` | draft intent + queue from an existing codebase |
| `kvist import REPO_URL [--branch BR] [--component PATH] [DEST_DIR]` | import a Kvist component from Git |
| `kvist vendor [PROJECT_DIR] [--vendored-dir PATH]` | populate offline vendored dependencies (ADR-0011) |
| `kvist toolchain [PROJECT_DIR]` | provision the pinned Rust toolchain on the host (ADR-0012) |
| `kvist task <...>` | work the queue (below) |
| `kvist component <...>` | manage intent (below) |
| `kvist agent <...>` | configure models/roles (below) |
| `kvist prompt [PROMPT] [options]` | run a free-form supervised prompt |
| `kvist completions SHELL` | shell completion scripts |
| `kvist authoring-apply` | *(hidden)* internal effect-applier entry point |

### 4.2 `kvist task`

All component-scoped forms take an optional `COMPONENT_DIR` (P2):

| Command | Form | Purpose |
| ------- | ---- | ------- |
| `task next` | `kvist task next [COMPONENT_DIR]` | print the first ready task (no state change) |
| `task run` | `kvist task run [COMPONENT_DIR] [TASK_ID] [--stream]` | execute one task with the supervised agent; omitted task = suggest the next ready one (confirmed interactively; non-interactive contexts fail clearly) |
| `task log` | `kvist task log [COMPONENT_DIR] TASK_ID` | print the most recent bounded, redacted execution log |
| `task replay` | `kvist task replay SESSION_JSONL [--max-turns N]` | replay a recorded trajectory |
| `task transition` | `kvist task transition [COMPONENT_DIR] TASK_ID STATUS [--reason R]` | one audited manual status transition |
| `task finalize` | `kvist task finalize [COMPONENT_DIR] TASK_ID ATTEMPT_ID accept\|block [--commit] [--reason R]` | human disposition of a completed attempt |
| `task recover` | `kvist task recover [COMPONENT_DIR] TASK_ID ATTEMPT_ID --disposition execution-did-not-start` | reconcile a fenced attempt with no execution evidence |
| `task approve-policy` | `kvist task approve-policy [PROJECT_DIR]` | approve the effective test-execution policy |
| `task unlock` | `kvist task unlock [COMPONENT_DIR] [--force]` | release a stale component lock |

### 4.3 `kvist component`

| Command | Form | Purpose |
| ------- | ---- | ------- |
| `component new` | `kvist component new COMPONENT_DIR` | scaffold `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md` |
| `component validate` | `kvist component validate [COMPONENT_DIR]` | validate the three intent documents |
| `component accept` | `kvist component accept [COMPONENT_DIR] [--commit] [--message M]` | record accepted revisions; `--commit` also creates the Git commit |
| `component commit` | `kvist component commit ACCEPTANCE_ID` | (re)perform the isolated index commit for a pending acceptance |

### 4.4 `kvist agent`

| Command | Form | Purpose |
| ------- | ---- | ------- |
| `agent setup` | `kvist agent setup [--force]` | wizard: discover, qualify, save a model profile |
| `agent list` | `kvist agent list` | profiles and their role assignments |
| `agent remove` | `kvist agent remove [MODEL_NAME] [--all] [--global]` | remove profile(s) |
| `agent check` | `kvist agent check [--global]` | live-verify configured profiles |
| `agent role` | `kvist agent role [list\|set ROLE MODEL [--effort E]\|clear ROLE [--all]] [--global]` | bind roles (developer, architect, security-reviewer) to profiles |

## 5. Context resolution rules (normative)

1. `resolve_project(explicit)`:
   - explicit `PROJECT_DIR` given: resolve it against the current directory;
     the command proceeds as today (a missing `kvist.toml` yields the
     existing `ProjectConfigurationMissing` error).
   - omitted: walk upward from the current directory, at most 32 levels, to
     the nearest directory containing a regular `kvist.toml`. Found → that
     is the project root. Not found → error:
     `not inside a Kvist project (no kvist.toml found above the current
     directory)` with hint: `run 'kvist init <DIR>' to create a project,
     or pass an explicit PROJECT_DIR`.
2. `resolve_component(project, explicit)`:
   - explicit: normalize exactly as before (relative to the component root;
     `.` selects the root component; absolute paths outside the project are
     rejected).
   - omitted: let `cwd` be the current directory.
     - If `cwd` is not at or below the project root: error naming the
       resolved project and hinting `cd` into a component or passing an
       explicit COMPONENT_DIR.
     - Let `rel` be `cwd` relative to the component root, when possible.
       Select the deepest discovered component whose path equals `rel` or
       is an ancestor of it. If none: if the root component `.` is a
       discovered component and `cwd` is at or below the component root,
       select `.`; otherwise error listing the known components.
3. `kvist` with no arguments: resolve the project with rule 1 (omitted).
   Success → render the overview and a footer: `tip: run 'kvist help' for
   all commands`. Failure (not in a project) → print the top-level help
   with exit status 0.
4. The interactive shell's `cd [COMPONENT]` builtin sets the session focus.
   When a dispatched command has an omitted optional component argument and
   a focus is set, the shell injects the focus value before execution.
   Explicit component arguments in the typed command always win, with the
   existing one-line reminder.

## 6. Output and guidance rules (normative)

1. Every Kvist domain error that arises from project state (not parser
   syntax) ends with a blank line + `hint:` + the concrete unblocking
   action, where one exists. Hints name real commands and, where needed,
   the exact component path involved.
2. `kvist status` (overview) shows, per component: state, document
   validity summary, task progress, next ready task, and an **Action**
   line for every non-current state. Stale components additionally list
   each changed document (expected vs observed revision) and a diff hint.
3. Gating success messages chain to the next lifecycle stage
   (`accept` → `task next`; `run` → `task log` / `task finalize`;
   `approve-policy` → `task run`).
4. Nothing may require a TTY on the non-interactive path: no pager, no
   prompts; confirmations degrade to a clear failure with the command that
   expresses the intent explicitly.

## 7. Non-goals

- No new persistent state: resolution is derived from the filesystem on
  every invocation; nothing is cached.
- No project auto-detection beyond the `kvist.toml` marker: Kvist still
  never searches *sideways*, and an explicit path always wins.
- No change to durable artifact schemas, queue semantics, sandboxing, or
  VCS policy. This design changes the surface, context resolution, and
  guidance only.
- No aliases: if two commands would be convenient, the answer is better
  context resolution, not a second verb.
