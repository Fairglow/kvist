# Kvist Command-Set Design

This document is the authoritative design for the Kvist command-line surface.
It explains the core ideas behind the commands, how they fit together into one
workflow, and the rules that keep the interface simple, predictable, and
self-explanatory. Implementation in `maerg/` MUST follow this document.

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
**stale**: Kvist records _which_ document changed and what the expected and
observed revisions are, and every surface (status, errors) tells the user to
review the change and run `kvist component accept` again.

## 3. Design principles

### P1 — One verb per lifecycle stage

There is one canonical command for each stage of the lifecycle and for each
cross-cutting concern. No aliases, no overlapping subgroups:

| Concern             | Canonical command                                        |
| ------------------- | -------------------------------------------------------- |
| interactive work    | `kvist`                                                  |
| inspect state       | `kvist status` (default view), `tree`, `doctor`          |
| recover a project   | `kvist repair`                                           |
| create component    | `kvist component new`                                    |
| check intent docs   | `kvist component validate`                               |
| accept intent       | `kvist component accept`                                 |
| pick a task         | `kvist task next`                                        |
| execute a task      | `kvist task run`                                         |
| inspect execution   | `kvist task log` / `task replay`                         |
| human disposition   | `kvist task finalize` / `task recover`                   |
| manual state        | `kvist task transition`                                  |
| policy gate         | `kvist task approve-policy`                              |
| lock maintenance    | `kvist task unlock`                                      |
| agent configuration | `kvist agent setup\|list\|remove\|check\|role`           |
| onboarding          | `kvist init` / `convert` / `reverse-discover` / `import` |
| offline build       | `kvist vendor` / `kvist toolchain`                       |

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
  `cd maerg`, every dispatched command — `task next`, `component
validate`, `task run`, ... — targets `maerg` without repetition. The
  prompt shows the focus, and typing a command that names a _different_
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
  (e.g. `git diff HEAD -- <path>` under Git) to see _what_ changed.
- **Bare `kvist`** (no arguments) opens the interactive workspace when
  stdin/stdout are terminals, at the resolved project or current directory
  before initialization. Redirected invocation remains a finite Markdown
  overview, or guided help outside a project. Global `--json` never opens a
  workspace.
- **Success messages** for gating commands name the next stage: after
  `component accept`, the output reports whether the component is current or
  remains blocked and names `task next` or the blocked-task transition; after a run finishes, it points at `task log` and
  `task finalize`.

When stale intent and an unresolved attempt coexist, first inspect the attempt
and explicitly reject it with `task finalize COMPONENT TASK ATTEMPT block
--reason TEXT`. This remains available while stale and does not certify effects
or accept intent. Then review/accept intent before returning the task to
pending. Execution and result acceptance remain gated; never delete evidence
or use acceptance merely to bypass recovery.

### P4 — Help is a tour

- Top-level `kvist --help` leads with the typical flow (five commands)
  before listing the surface; every command's `about` text says _when to
  use it_, not just what it does.
- `kvist help [TOPIC]` renders the guided tour (core concepts, lifecycle)
  plus closed-set topics — `concepts`, `lifecycle`, `task-states` (the
  durable task states, the legal transitions, and the command that achieves
  each). Topics are tab-completable. Every topic ends with a pointer to the
  deeper documentation (`GUIDE.md`, `docs/command-set.md`, a component's
  `CONTRACT.md`).
- The shell's `help [TOPIC]` builtin shares the same renderer: builtins and
  workflow commands with their focus-aware forms (no component argument
  needed after `cd`), and the same topics with completion.
- `kvist completions <SHELL>` remains for tab completion outside the shell.

### P5 — Explicit stays deterministic

Explicit arguments are always accepted and win over context resolution.
`--json` remains global and stable. The stable text report
(`status --format text`) keeps its `status-format-version`. JSON outputs
keep their documented shapes. Nothing interactive is ever required of a
script: no confirmation, no pager, no terminal.

## 4. The command surface

### 4.1 Top level

| Command                                                                                                                                                                                | Form                                                                                                                                                                                                              | Purpose (when to use)                                       |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `kvist`                                                                                                                                                                                | _(no args)_                                                                                                                                                                                                       | Open the workspace; redirected output is a finite overview. |
| `kvist shell [PROJECT_DIR]`                                                                                                                                                            | explicit project selection for the same workspace                                                                                                                                                                 |
| `kvist status [COMPONENT_DIR] [--project PROJECT_DIR] [--details] [--format overview\|text\|json] [--only-documents] [--only-impls] [--unfinished]`                                    | One project table row per component; exact-component details or `--details` for the whole project; stable text/json for scripts. Positional project directories remain supported; `.` selects the root component. |
| `kvist tree [PROJECT_DIR]`                                                                                                                                                             | browse the component hierarchy                                                                                                                                                                                    |
| `kvist doctor [PROJECT_DIR]`                                                                                                                                                           | verify root artifacts, VCS, versions (read-only health check)                                                                                                                                                     |
| `kvist repair [PROJECT_DIR] [--dry-run] [--salvage] [--reset PATH]... [--clean-slate] [--component-root PATH] [--backup-dir PATH] [--approve SHA256] [--ai PROFILE] [--proposal PATH]` | default preservation-first salvage planning, valid blocked drafts, optional frozen AI proposals, and exactly approved backed-up resets; see [recovery](recovery.md)                                               |
| `kvist init [PROJECT_DIR]`                                                                                                                                                             | start a new project here                                                                                                                                                                                          |
| `kvist convert PROJECT_DIR`                                                                                                                                                            | onboard an existing Rust crate (drafts in `.kvist/`)                                                                                                                                                              |
| `kvist reverse-discover PATH`                                                                                                                                                          | draft intent + queue from an existing codebase                                                                                                                                                                    |
| `kvist import REPO_URL [--branch BR] [--component PATH] [DEST_DIR]`                                                                                                                    | import a Kvist component from Git                                                                                                                                                                                 |
| `kvist vendor [PROJECT_DIR] [--vendored-dir PATH]`                                                                                                                                     | prepare native offline caches incrementally; explicit Rust directory export only with `--vendored-dir` (ADR-0020)                                                                                                    |
| `kvist toolchain [PROJECT_DIR]`                                                                                                                                                        | provision the pinned Rust toolchain on the host (ADR-0012)                                                                                                                                                        |
| `kvist task <...>`                                                                                                                                                                     | work the queue (below)                                                                                                                                                                                            |
| `kvist component <...>`                                                                                                                                                                | manage intent (below)                                                                                                                                                                                             |
| `kvist agent <...>`                                                                                                                                                                    | configure models/roles (below)                                                                                                                                                                                    |
| `kvist prompt [PROMPT] [options]`                                                                                                                                                      | run a free-form supervised prompt                                                                                                                                                                                 |
| `kvist help [TOPIC]`                                                                                                                                                                   | guided tour; TOPIC: `concepts`, `lifecycle`, `task-states` (tab-completable)                                                                                                                                      |
| `kvist completions SHELL`                                                                                                                                                              | shell completion scripts                                                                                                                                                                                          |
| `kvist authoring-apply`                                                                                                                                                                | _(hidden)_ internal effect-applier entry point                                                                                                                                                                    |

Configured Rust vendoring validates the selected workspace/path-dependency tree
before fetching. Source links and special entries (including Unix control
sockets and FIFOs) are rejected, with the offending path and type in the error.
Stop the owning process before moving runtime data outside the selected source
tree; do not delete live sockets or disable the sandbox to bypass validation.
Malformed manifests/locks and filesystem failures retain their native cause.
A configured acquisition failure never falls back to host acquisition.

### 4.2 `kvist task`

All component-scoped forms take an optional `COMPONENT_DIR` (P2):

| Command               | Form                                                                                           | Purpose                                                                                                                                                                                                                                                                                                               |
| --------------------- | ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `task next`           | `kvist task next [COMPONENT_DIR]`                                                              | print the first ready task (no state change)                                                                                                                                                                                                                                                                          |
| `task run`            | `kvist task run COMPONENT_DIR [TASK_ID\|N] [--stream]`                                         | omit the target for continuous dependency-ready work with per-item local checkpoint commits; an exact TASK_ID runs only that task; a bare positive integer N runs the Nth ready task (1-based, matching the numbered `tasks` listing); stop for final approval, decisions, or blockers, without per-task confirmation |
| `task log`            | `kvist task log [COMPONENT_DIR] TASK_ID`                                                       | print the most recent bounded, redacted execution log                                                                                                                                                                                                                                                                 |
| `task replay`         | `kvist task replay SESSION_JSONL [--max-turns N]`                                              | replay a recorded trajectory                                                                                                                                                                                                                                                                                          |
| `task transition`     | `kvist task transition [COMPONENT_DIR] TASK_ID STATUS [--reason R]`                            | one audited manual status transition                                                                                                                                                                                                                                                                                  |
| `task finalize`       | `kvist task finalize [COMPONENT_DIR] TASK_ID ATTEMPT_ID accept\|block\|reset [--commit] [--reason R]` | human disposition of a completed attempt                                                                                                                                                                                                                                                                              |
| `task recover`        | `kvist task recover [COMPONENT_DIR] TASK_ID ATTEMPT_ID --disposition execution-did-not-start`  | reconcile a fenced attempt with no execution evidence                                                                                                                                                                                                                                                                 |
| `task approve-policy` | `kvist task approve-policy [PROJECT_DIR]`                                                      | approve the effective test-execution policy                                                                                                                                                                                                                                                                           |
| `task unlock`         | `kvist task unlock [COMPONENT_DIR] [--force]`                                                  | release a stale component lock                                                                                                                                                                                                                                                                                        |

### 4.3 `kvist component`

| Command              | Form                                                              | Purpose                                                           |
| -------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `component new`      | `kvist component new COMPONENT_DIR`                               | scaffold `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`            |
| `component validate` | `kvist component validate [COMPONENT_DIR]`                        | validate the three intent documents                               |
| `component accept`   | `kvist component accept [COMPONENT_DIR] [--commit] [--message M]` | record accepted revisions; `--commit` also creates the Git commit |
| `component commit`   | `kvist component commit ACCEPTANCE_ID`                            | (re)perform the isolated index commit for a pending acceptance    |

### 4.4 `kvist agent`

| Command              | Form                                                                                                               | Purpose                                                                 |
| -------------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| `agent setup`        | `kvist agent setup [--force]`                                                                                      | wizard: discover, qualify, save a model profile                         |
| `agent list`         | `kvist agent list`                                                                                                 | profiles and their role assignments                                     |
| `agent show`         | `kvist agent show MODEL_NAME [--registry PATH]`                                                                    | offline credential-free details, references, source digest and fallback |
| `agent enable`       | `kvist agent enable MODEL_NAME --registry PATH --expected-revision DIGEST [--global]`                              | enable exactly one inspected registry definition                        |
| `agent disable`      | `kvist agent disable MODEL_NAME --registry PATH --expected-revision DIGEST [--global]`                             | disable an unreferenced registry definition                             |
| `agent remove`       | `kvist agent remove MODEL_NAME [--registry PATH --expected-revision DIGEST] [--global]`                            | remove an unreferenced explicit registry or config-local definition     |
| `agent remove --all` | `kvist agent remove --all [--global]`                                                                              | reset local agent configuration, never the shared registry              |
| `agent check`        | `kvist agent check [--global]`                                                                                     | live-verify configured profiles                                         |
| `agent role`         | `kvist agent role [list\|set ROLE MODEL [--thinking-effort E \| --inherit-effort]\|clear ROLE [--all]] [--global]` | inspect/edit developer, architect and security-reviewer assignments     |

Config-local removal and role set/clear accept `--expected-revision DIGEST`.
Role set additionally accepts `--profile-source PATH --profile-revision DIGEST`.
The TUI supplies these source/revision pins from offline inspection. Registry
mutations require the exact source and its digest from `agent show`; referenced
definitions must be unassigned first. Add/check remain explicit host-execution
acknowledgements, not task-policy approval.

### 4.5 The interactive TUI and Markdown-first output

Bare `kvist` opens a full-screen terminal UI in the shape of the `skott`
layout, not a line-oriented REPL:

- **Status heading.** Shows the active component together with its state and
  task progress, plus the VCS branch, sandbox backend, default model, and
  live/stale lock counts. When no component is selected the heading shows the
  whole project and commands operate on the project; when a component is
  selected it is auto-selected as the default for every command that expects a
  component, so the operator is never asked for it and it is pre-filled.
- **Main transcript area.** A bounded on-screen window with a scrollbar over a
  durable, per-component, disk-persisted record of every interaction (commands
  and their rendered Markdown output). The record is unlimited in size and
  splits into successive files when one grows too large. The operator can
  scroll back through the session. Switching component clears the transcript
  area and restores that component's former transcript when the operator
  returns to it.
- **Command footer.** The same command surface as the CLI with full
  tab-completion and keyboard-shortcut help rendered the same way `skott` does
  it.

**Markdown is the canonical output.** Every command reports a well-formed
Markdown document as its canonical result, and the TUI renders that document
with the `blad` renderer so tables, diagrams, math, and code blocks display
with full fidelity. The stable `status --format text` and `--format json`
report forms are unchanged for non-interactive consumers, and the plain-text
degradation contract still applies when the terminal cannot host the TUI.
Global `--json`, generated completion scripts and internal effect protocols
retain their native formats. Agent/tool JSON objects become readable metadata,
tables and decoded Markdown report bodies in the transcript.

The host owns terminal drawing; child commands use isolated pipes, never a
nested terminal UI. Input while a command runs supplies its stdin; answers are
masked and excluded from history. Ctrl+C cancels the owned command without
claiming rollback. Ctrl+D on empty input closes a running command's stdin (EOF)
or exits when idle. Ctrl+Q and menu Quit share idle exit and an explicit
cancel-and-quit confirmation while a command runs, followed by reaping.
F2 opens the root menu from every overlay and cancels, never approves, a staged
confirmation. Persisted settings (including appearance and pins) require an idle
command; read-only inspection and transcript navigation remain available.
`exit` also quits when idle. Host-execution prompt opt-outs require the
standalone interactive command, not the embedded workspace.

| Key                            | Action                                                              |
| ------------------------------ | ------------------------------------------------------------------- |
| Tab / Shift+Tab                | Open/cycle completions; Enter accepts a selection                   |
| Up / Down                      | Command recall (or completion/menu selection)                       |
| F1 / F2                        | Scrollable Markdown shortcut help / unified workspace menu          |
| PageUp / PageDown; mouse wheel | Browse transcript, including older segments                         |
| Ctrl+Home / Ctrl+End           | Oldest history / resume following latest output                     |
| Alt+Left / Alt+Right           | Horizontal transcript scrolling                                     |
| Escape                         | Dismiss completion; go back one menu level; otherwise open the menu |
| Ctrl+C / Ctrl+D / Ctrl+Q       | Cancel or clear input / EOF (idle: exit) / shared Quit action       |
| Ctrl+P in Components           | Pin/unpin the highlighted component                                 |

History is private local diagnostic state at `.kvist/transcript/` for the
project and each selected component. Numbered Markdown pages target 1 MiB,
rotate between top-level blocks, and allow an individual block up to 16 MiB;
larger blocks fail capture explicitly. No total retention limit deletes older
pages. The global scrollbar allocates one band per segment, with local line
precision. Only the current bounded page is rendered; cross-page reference
links/footnotes are not a whole-history document. Command recall retains at
most 500 entries. A competing writer, unsafe file or recording error fails
explicitly. Do not treat transcript text as execution or compliance evidence.
Unsupported/small terminals, `NO_COLOR`, `CLICOLOR=0` or `TERM=dumb` retain the
line-oriented fallback.

**Workspace menu.** Esc and F2 open Components, Role assignments, Models,
Appearance, History, Help and Quit; Cancel command is available during work.
Esc returns one level and preserves parent cursor/filter and command draft.
Current choices are preselected and independently marked. Type to filter
component/model lists. Components has bounded recent/pinned shortcuts instead
of an unbounded tab strip and retains `cd` focus/prefill semantics.
Role settings expose all predefined roles and project/user scope; inherited
effort is distinct from explicit `none`. Changes retain task-role and policy
authority and wait until active execution finishes.

Models supplies local details plus explicit Add/check and source/revision-pinned
enable/disable/remove actions. Inspection never contacts providers; discovery
and qualification require acknowledged host authority and may incur billing.
Mutations refuse references/stale definitions and disclose shared-consumer and
fallback effects. Dark/light RGB palettes independently style transcript,
raised/selected surfaces and Blad with measured contrast.

**Run the whole component.** Bare `task run COMPONENT_DIR` iterates the
component's tasks in dependency order, running each ready task, and stops when
the component is complete, when a task becomes blocked, or when a task reaches
an awaiting-decision point. It never auto-resolves a decision or unblocks a
blocked task; it pauses and reports the exact state and the command that
resolves it, leaving durable task state for explicit finalize or recovery.

**Ready work and numbered runs.** The shell `tasks` builtin lists only the
ready (schedulable) tasks, numbered in queue order so row 1 is the next task,
with the full count always shown; `tasks --all` (or an explicit `--status`) lists
every task with its state. A bare positive integer targets the Nth ready task:
`run 1` runs the next task, `run 3` the third ready task. On the command line
the component is explicit (`kvist task run COMPONENT_DIR 3`); in the shell it
comes from focus. An exact task ID always wins over a number, so an all-digit
ID is never mistaken for a position.

The default `[task_run] commit = "item"` checkpoints each complete TODO
item, waiting for all its queued test, implementation and review stages. Item IDs use
the shared prefix before those lifecycle suffixes, and item tasks must be
contiguous. `"component"` checkpoints once at the end; `"manual"` leaves commits
to the operator. Policy changes require `kvist task approve-policy`.
Automatic commits require an attached Git branch, existing HEAD, configured
identity/signing and clean overlapping paths. Unrelated staged/unstaged changes
are preserved. Commits are local unaccepted checkpoints, never pushes.
On a checkpoint failure, fix the reported Git/signing problem and repeat bare
run; retained completed tasks are not executed again. Decisions/failed or
interrupted effects retain existing explicit resolution/recovery requirements.
Final execution awaits human approval; `component accept` still accepts intent,
not implementation results.

Task decisions and blockers name a human-readable Markdown brief under the
component's `.kvist/attention/`. It retains agent explanations when available
and host instructions for choosing, clarifying, rejecting and safely resuming.
Review JSON has a same-stem `.md` companion; `component.md` indexes final reports.
Open the printed file manually until transcript rendering is complete.
`kvist decision COMPONENT_DIR` inspects open pauses. `options [--task TASK_ID]`
creates stable advisory choices; `discuss MESSAGE [--task TASK_ID]` retains
follow-up questions without controlled edits. `plan --option OPTION_ID` or
`plan --guidance TEXT` stages the entire typed intent/task change-set.
`plan --import PATH --review-exception REASON` provides the same manual/local
route. A separate intent review or explicit exception accompanies each plan.
Inspect its complete Markdown preview, then invoke the displayed short
`do NUMBER` response. Options, apply-only, and apply-and-continue have distinct
component-scoped numbers and full command expansions; Tab offers their
descriptions. `actions` lists them and discovers existing authenticated plans
without inference. Numbers are retained, not recycled; stale or tampered
commands are refused, never redirected. The advanced
`apply PLAN_ID --approve SHA256 [--continue]` remains available. Apply calls no model and
creates no Git commit. It validates frozen inputs, owns queue metadata and
reopens only the selected resolved pause with approved prerequisites wired first.
Changed inputs invalidate staged approval; interrupted application fences ordinary
writers until the exact command completes. Withdrawal archives the full explicitly
selected uncompleted dependant closure with reasons, never fake completion.
Unknown attempts still require recovery. See GUIDE.md's decisions and blockers
section and maerg/CONTRACT.md for the strict native JSON version-1 draft.

With shell component focus active, component arguments are visibly prefilled
in the editable buffer when their position is reached. Delete the inserted
argument to choose another component; task-ID completion then follows it.

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
   existing one-line reminder. The shell's `help [TOPIC]` builtin offers the
   same topics as `kvist help [TOPIC]` with tab completion.

## 6. Output and guidance rules (normative)

1. Every Kvist domain error that arises from project state (not parser
   syntax) ends with a blank line + `hint:` + the concrete unblocking
   action, where one exists. Hints name real commands and, where needed,
   the exact component path involved. An invalid `task transition` reports
   the legal transitions from the current state plus a pointer to
   `kvist help task-states`.
2. `kvist status` never shows a non-current project state bare: all three
   formats list every non-valid root artifact with its exact reason and a
   concrete action (`kvist init` for uninitialized projects, `kvist repair`
   for normalization and proposed salvage/reset recovery).
   Per component, the overview and text forms show state, document validity
   summary, task progress, next ready task, and an **Action** line for every
   non-current state. Stale components additionally list
   each changed document (expected vs observed revision) and a diff hint.
   Blocked components list each blocked task with its full first-line
   `blocked_reason` and the exact non-executing reopening command
   (`task transition … pending`, with an incomplete-dependency count when
   applicable); awaiting-decision tasks are listed with a
   pointer to `kvist help task-states`.
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
  never searches _sideways_, and an explicit path always wins.
- No change to durable artifact schemas, queue semantics, sandboxing, or
  VCS policy. This design changes the surface, context resolution, and
  guidance only.
- No aliases: if two commands would be convenient, the answer is better
  context resolution, not a second verb.
