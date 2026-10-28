# ADR 0016: Markdown-first output and the Blad-rendered interactive TUI

## Status

Proposed. This decision is not an implementation record, acceptance receipt, or
compliance certification. It is approved for planning and must be accepted by
the human architect before the dependent implementation work is promoted.

## Context

The interactive `kvist shell` is a line-oriented reedline REPL: each command
prints its result to the terminal, the result scrolls off the visible screen,
and the operator must re-run `status` or `tasks` to re-read state. Output is
plain text that the shell soft-wraps and pages, so tables, diagrams, and other
structured content are not rendered with the fidelity the project already ships
in the `blad` Markdown renderer.

The project already owns a terminal Markdown renderer (`blad`, package
`kvist-blad`) with tables, strikethrough, task lists, fenced code with syntax
highlighting, math, Mermaid text diagrams, and themes, exposed as a finite
string-in/writer-out `MarkdownRenderer`. The standalone `skott` workspace agent
already demonstrates the target presentation: a status header, a scrollable
transcript panel with a proportional scrollbar, and a command footer, plus an
overlay settings menu. Kvist's own shell does not reuse any of this.

Productivity is a primary aim of Kvist. Re-reading state, losing earlier output
off-screen, and rendering the same facts as unstyled text all tax the operator.
The renderer, the TUI pattern, and the command surface already exist; they are
not integrated.

## Decision

**Markdown is the canonical output format for every Kvist command.** Each
command produces a well-formed Markdown document describing its result: state,
validity, task progress, the next ready task, actionable next steps, blocked or
awaiting-decision reasons, and stale-document revision details. The Markdown is
the single source of truth for what a command reports.

**The interactive shell becomes a full-screen terminal UI (TUI)** in the shape
of the `skott` layout:

- A **status heading** that shows the active component together with its state
  and task progress, plus the VCS branch, sandbox backend, default model, and
  live/stale lock counts. When no component is selected the heading shows the
  whole project and the shell operates on the project; when a component is
  selected it is auto-selected as the default for every command that expects a
  component, so the operator is never asked for it and it is pre-filled.
- A **main transcript area** with a scrollbar that shows every interaction with
  Kvist (commands and their rendered Markdown output) and lets the operator
  scroll back through the session. The on-screen window is bounded; the
  transcript is persisted to disk per component, unlimited in size, and split
  into successive files when one grows too large. Switching component clears
  the transcript area and restores that component's former transcript when the
  operator returns to it.
- A **command footer** with the same command surface as the CLI and full
  tab-completion (static from the clap surface, dynamic from the project
  state), plus keyboard-shortcut help rendered the same way `skott` does it.

**All command output is rendered with `blad`.** The TUI embeds the `blad`
`MarkdownRenderer` to render each command's Markdown into the transcript, so
tables, diagrams, math, and code blocks display with full fidelity. `maerg`
gains a dependency on `blad` (as `skott` already has) and on `ratatui` for the
TUI layout. The renderer is a pure function of its inputs plus the theme, so
styled and plain output remain unit-testable without a terminal.

**A settings menu** (the `skott` overlay pattern) exposes the major choices:
model, reasoning effort, session history, component selection, theme, and other
settings. Selecting a component from the menu is the same focus change as the
`cd` builtin.

**A "run the whole component" command** iterates over the component's tasks in
dependency order, running each ready task, and stops when the component is
complete, when a task becomes blocked, or when a task reaches an
awaiting-decision point. It never auto-resolves a decision or unblocks a
blocked task; it pauses and reports the exact state and the command that
resolves it, so the operator can decide and resume.

## Alternatives

- **Keep the line-oriented shell and only add a pager.** This preserves the
  status quo and does not render Markdown, does not persist a scrollable
  transcript, and does not provide a settings menu or whole-component run. It
  leaves the productivity gap in place.
- **Render Markdown by spawning the `blad` executable as a subprocess per
  command.** This adds a process boundary, loses streaming, and cannot share
  the TUI's theme, width, and panel background with the renderer. Embedding the
  library is simpler and faster.
- **Re-use the `skott` TUI crate directly.** `skott` is a workspace agent with
  its own session, model, and host-execution concerns; importing it into
  `maerg` would couple the protected task authority to the workspace-agent
  presentation. Sharing the `blad` renderer and the TUI *pattern*, while
  keeping `maerg`'s own shell, is the cleaner boundary.
- **Make Markdown the only output and drop the stable text/JSON report forms.**
  Scripts and automation depend on the stable `status --format text` and
  `--format json` shapes. Markdown is the canonical *interactive* output; the
  stable report forms remain for non-interactive consumers.

## Delivery and verification

Define the local requirements, contract, design, and ordered tasks before code
(see `maerg/TODOS.yaml`). The work is delivered in independently verified
chains: the Markdown output layer, the TUI layout with the persisted
per-component transcript and scrollbar, the settings menu with component
auto-selection, the keyboard-shortcut help, and the whole-component run
command. Each chain follows the test → implementation → security-audit →
compliance-review lifecycle.

Rendering, transcript persistence and splitting, component focus restoration,
settings persistence, and the run-to-completion/blocker/decision loop are
covered by unit and integration tests. The stable text and JSON report forms
are unchanged, so scripts are unaffected. The plain-text degradation contract
of `REQ-INTERACTIVE-SHELL` still applies: when the terminal cannot host the TUI
or styling is disabled, output degrades to plain Markdown text.

No generated receipt or acceptance is implied by this ADR. Clean-slate source
observation and a separate source-blind intent comparison remain necessary
before the finished shell is promoted.
