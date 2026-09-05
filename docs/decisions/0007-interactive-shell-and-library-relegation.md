# ADR 0007: Interactive Shell and Engine Library Relegation

## Status

Proposed

## Context

Kvist currently operates as a stateless command-line tool where every operation runs as a short-lived process. For multi-step, interactive workflows—such as analyzing requirements, editing prompt templates, running iterative sandbox tasks, and performing reviews—the stateless model incurs heavy overhead. The operator must repeatedly type out complete commands, recall specific task IDs, and re-read terminal contexts.

There is a need for a unified, interactive workspace environment (a REPL shell) to provide real-time feedback, dynamic state visualization, and fluid navigation. In this interface, commands must remain the primary focus. Initiating a command should not require an artificial prefix (like `/`), and raw prompt authoring should be treated as an explicit, high-signal action rather than the default text input to avoid accidental submissions.

Furthermore, building this interactive interface inside a stateless CLI codebase would lead to tightly coupled, hard-to-maintain code. To solve this cleanly, the Kvist engine must be refactored into a reusable core library, allowing the interactive shell to become the primary, first-class application frontend.

## Decision

Relegate the core engine (`kvist`) to a library role (`kvist_core`), separating the domain logic (project state, candidate discovery, task queue management, sandboxed execution, and VCS commits) from command-line presentation. The CLI binary will act as a thin frontend wrapper around this core library, sharing it with the new interactive shell subsystem.

Introduce an interactive REPL workspace interface, invoked via `kvist shell` (or as the default interactive mode). 

### UI/UX Rules and Input Behavior

* **Commands as Primary:** Standard user inputs are parsed directly as commands (e.g. `task run`, `component validate`). No prefix (such as `/` or `:`) is required. 
* **Tab-Completion:** Provide comprehensive tab-completion for all command verbs, option flags, component paths, model profiles, and queue task IDs, dynamically resolved from the library's active project state.
* **Inline Help & Status Bar:** Render a styled, non-intrusive status bar displaying the current workspace context (active VCS branch, selected model profile, sandbox backend, and active locks). Show dimmed parameter hints inline as the user types.
* **Prompt Authoring via Editor:** Raw prompts or multi-line strings are not typed directly in the command prompt. Instead, typing `prompt <task_id>` (or a shortcut key like `Ctrl+P`) launches an embedded or external editor session (respecting `$EDITOR`, defaulting to standard micro-editors like `nano`, `vim`, or `vi`). Once saved and exited, the prompt is validated, displayed as a formatted block, and submitted.
* **Graceful Exit:** Support standard EOF (`Ctrl+D`) and explicit `exit`/`quit` commands to cleanly return the operator to the host shell.

### Output Streaming and Pagination

* **Transient Streams and Link Replacement:** During sandboxed model execution, stream the output chunks dynamically to standard error alongside a transient progress spinner. Once execution completes, use ANSI escape sequences to clear the raw intermediate stream chunks from the terminal, replacing them with:
  1. The original prompt block (collapsed or highlighted).
  2. A clickable link to the raw execution log (e.g., `Logs: ./.kvist/logs/...`).
  3. The clean, finalized command result.
* **Paginated Terminal Output:** Integrate a terminal pager wrapper (such as the `minus` crate or raw piping to `less -FRX`) for all terminal output streams. This guarantees that large component trees, long logs, and complex status tables are paginated cleanly on the screen without truncating colors or overflowing screen buffers.
* **Clean Session Logging:** Maintain an active session journal of the REPL shell. Only log final command inputs, execution results, and log links to the session file; transient stream chunks and progress states are strictly excluded from the permanent session journal.

## Alternatives considered

* **Default to prompt input with slash commands:** Making raw input write prompts directly and requiring `/` for commands (e.g. `/task run`). Rejected because Kvist is an architecture-driven, command-focused workflow engine, not a generic chat assistant. Commands must remain the first-class, un-prefixed focus of the shell interface.
* **Custom inline multi-line text editor:** Building a custom terminal text editor within the prompt input field. Rejected as terminal line-editors are complex to implement robustly across diverse platforms, terminals, and keybinding configurations. Delegating to the system's `$EDITOR` is idiomatic, robust, and preserves the operator's personal environment preferences (such as Vim/Emacs keys).
* **Stateless execution wrappers:** Keeping the engine as an independent binary and running shell commands through process spawning. Rejected due to the substantial latency and lack of rich, in-memory caching of workspace state, component configurations, and active sessions.

## Consequences

* The Kvist codebase is refactored into a highly modular library-and-frontend architecture, making core operations easy to test, embed, or expose in alternate interfaces (like GUI or web dashboards) in the future.
* Operators gain a powerful, persistent workspace environment with real-time status updates and robust process feedback.
* Spawning and execution overhead is reduced through in-memory state tracking, and output legibility is significantly improved via integrated pagination and clear stream cleanup.
