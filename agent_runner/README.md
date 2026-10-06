# agent-runner

A first-class, interactive agent shell for Kvist. `agent-runner` lets you talk to
an AI coding agent in a modern terminal UI while the agent does real work, in the
style of `gemini` or GitHub `copilot`. Every tool the agent runs executes inside
the independently installed Bubblewrap sandbox, so it never asks for per-action
permission — the tools, working directory, and authority boundaries are declared
once, in configuration.

It is a standalone binary (`agent-runner`) that is independent of the `kvist` CLI
but shares the `agent-runtime` bounded mechanisms and the `kvist-sandbox-runner`
enforcement boundary, so it can also be launched from Kvist.

## Quick start

```
agent-runner                       # open the UI in the current directory
agent-runner --config ./cfg.toml   # use a specific configuration
agent-runner -m ollama "explain this repo"   # pick a model and submit a prompt
agent-runner --list-models         # print configured models and exit
```

Run it from an interactive terminal. It refuses non-interactive input with an
actionable message.

## Configuration

Configuration is TOML. See [config.example.toml](./config.example.toml) for a
complete, commented example. Key points:

- `schema_version` must be `1`.
- `models` declares the selectable models and their provider (`llama-server` or
  `ollama`), base URL, provider model name, and a per-turn deadline. Serving
  context is discovered for the selected model, not assumed to be 8192 tokens.
  Set per-model `context_limit` when the server cannot report its actual window
  (including an unloaded Ollama model). `response_reserve` controls the initial
  generation budget; by default it is up to 8192 tokens or a quarter of a small
  window. CLI budget overrides take precedence.
- `sandbox.runner` and `sandbox.backend` point at the `kvist-sandbox-runner`
  executable and the Bubblewrap backend; they default to resolved system paths.
- `tool_policy` exposes the shell denylist and the sandbox write root; the safe
  minimum denylist is always enforced.
- `[tool_profiles]` advertises language tool-chains only against what reaches the
  sandbox: each profile (`python`, `rust`, `javascript`, `go`, `c`) is `on`,
  `auto`, or `off`. `auto` advertises when the interpreter is available; `on`
  fails startup if it is not; `off` never advertises. The `Generic` base is
  always present.

## Theming

The terminal UI is themed by a single colour table (`tui::theme`): a `Theme`
supplies every surface the shell draws — panels, edges, notes, stats,
scrollbar, menus, and the Markdown style table (headings, inline code, code
blocks, tables).

Themes are **plain TOML files, not compiled into the binary**, so you can add
or edit one without rebuilding. Two built-ins ship, embedded as a fallback:

- **`dark`** (default): black panels with the standard terminal background for
  all output, a muted warm-gray tint for reasoning, and a dark patch under
  highlighted code.
- **`light`**: white panels with the same structural cues recoloured for a
  light terminal, keeping the same contrast structure.

Drop a `themes/` directory next to your `config.toml` to customize: a file
named `dark.toml` or `light.toml` there overrides the matching built-in, and
any other `<name>.toml` adds a new theme, selectable without a rebuild. See
`agent_runner/themes/dark.toml` and `light.toml` for the full, documented
schema — every item lists an explicit colour, and surfaces that paint their
own background (panel, reasoning, scrollbar, menu, code blocks) require one.
Colours may be written as a named colour or numeric RGB interchangeably
(e.g. `"midnightblue"`, `"#1a1b26"`, `"rgb(26, 27, 38)"`), parsed against the
full CSS Color Module Level 4 palette via the `csscolorparser` crate — never
an invented or terminal-palette-dependent list.

Select a theme in the configuration with `theme = "dark"`, `theme = "light"`,
or the name of any file found in your `themes/` directory (unknown names fail
at load with the accepted list). The `--theme` flag overrides the
configuration for one run, the menu's **Theme** item opens a picker listing
every discovered theme, and **Ctrl+S** cycles through all of them live:
existing transcript rows are restyled in place while text and Markdown
highlighting are left untouched.

Layout cues are theme-independent and keep content scannable: the transcript
and prompt panels carry a single top edge (no side or bottom borders), the
transcript always shows an adaptive right-edge scrollbar whose thumb encodes
the current window into the full transcript, model reasoning is set apart by a
left edge (`▌`/`│`) plus its tint, and highlighted code keeps its indentation
plus the theme's code patch. The echoed prompt is distinct by its bold prompt
foreground alone.

## Session history

Open **Esc -> Session history** and select a session for read-only replay.
**Ctrl+Home / Ctrl+End** jump to the beginning / end; arrows, PageUp/PageDown
and the mouse wheel scroll, and **Esc** returns to the history list. Replay
uses the transcript's themed adaptive scrollbar, with its own reserved column
so wrapped text remains reachable. These boundary keys also work in the live
transcript: Ctrl+Home stops auto-follow, and Ctrl+End resumes it.

History currently displays bounded diagnostic `.log` text, not a structured
Markdown conversation or an executable checkpoint. Files above 5 MiB are not
listed, and individual recorded text items may be truncated at 64 KiB.
The [Markdown transcript proposal](../docs/proposals/agent-runner-markdown-transcripts.md)
describes shared rendering, folding, full re-theming and safe continuation;
those capabilities are proposed, not implemented.

Both sandboxed and explicitly unconfined agents are instructed that results
may use Markdown, including tables, lists, links, references and language-tagged
code fences. Live rendering already supports many CommonMark/GFM constructs;
this is not a claim of complete GitHub Markdown or every extension.

## Tools

The agent is offered a small, robust tool set that maps to sandbox-executed
commands: `shell`, `read_file`, `write_file`, `list_dir`, `find_files`,
`search_files`, and `edit_file`. Writes are confined
to the working directory (the sandbox write root); reading is lenient within the
sandbox view. The shell enforces a safe-by-default denylist over destructive
commands, and advertises only the language tool-chains (`python`, `rust`,
`javascript`, `go`, `c`) that actually reach the sandbox.

Reads use **byte offsets**, not line numbers: pass the returned `next_offset`
to continue. Omitting `offset` deliberately rereads page zero. Search accepts
one regular file or a directory and an optional literal `file_pattern`.
Search/discovery skip generated trees by default; `include_generated=true`
opts in, and coverage fields report skipped/excluded material. Binary process
output is represented by its size and digest rather than injected as escaped
executable bytes into the next prompt.

Compaction begins near 75% of the selected window and retains history according
to available capacity, preserving outstanding user goals and complete tool
groups. A length-limited generation can be regenerated with a larger bound
before any tools execute. Attempts share the prompt's time/token limits; a
truncated proposal or unknown-effect tool call is never replayed.
Summaries are bounded and lossy; older file references may be dropped when
fitting them to the available context. Context estimates are heuristic rather
than a guarantee of exact provider-token accounting.

For Rust, startup resolves an already installed standard rustup toolchain from
the workspace's `rust-toolchain.toml`/`rust-toolchain` pin or the user's default.
The concrete installation and trusted Cargo wrappers are mounted read-only,
without mounting the home directory, rustup configuration, Cargo credentials,
or host caches. Missing tools/components must be provisioned separately.
Workspace `.kvist/vendored` dependencies are copied into a bounded private
read-only snapshot; normal PATH Cargo forces `--offline --locked`, uses
sandbox-local source paths and private scratch. Provision a matching
`Cargo.lock` before building; a missing or stale lock fails without changing it.
Explicit alternate Cargo paths are not rewritten, and the standalone workspace
remains writable. Changes to the pin require restarting the session; changes to
vendored dependencies require reprovisioning on the host and restarting.

## Authority and safety

- No per-action prompts: authority is established once in configuration.
- Fail closed: a missing or unverified sandbox runner never triggers host
  execution.
- Unsafe Rust is permitted only where necessary and minimally scoped, with
  documented invariants, targeted verification, and independent review. The
  runtime's two signal-handler installation calls use unsafe; this is not a
  promise that every component is unsafe-free.
- The Authoring phase denies network; package managers are present for offline
  and local use. Networked acquisition is a planned, separate phase.

## Development

```
cargo build -p agent-runner
cargo test -p agent-runner
cargo clippy -p agent-runner --all-targets
```

The component follows the Kvist five-artifact model: [REQUIREMENTS.md](./REQUIREMENTS.md),
[CONTRACT.md](./CONTRACT.md), [DESIGN.md](./DESIGN.md), and [TODOS.yaml](./TODOS.yaml).
