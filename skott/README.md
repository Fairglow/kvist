# skott

A first-class, interactive agent shell. `skott` lets you talk to an AI coding
agent in a modern terminal UI while the agent does real work, in the style of
`gemini` or GitHub `copilot`. Every tool the agent runs executes inside the
independently installed Bubblewrap sandbox, so it never asks for per-action
permission — the tools, working directory, and authority boundaries are
declared once, in configuration.

It is a standalone binary (`skott`) that is independent of the `kvist` CLI:
it is driven by its own TOML configuration and needs no Kvist project. It
shares the `sav` bounded mechanisms and the `galla-runner` enforcement
boundary, so it can also be launched from Kvist (`kvist prompt` runs the same
shell). Its Markdown rendering is provided by the `blad` library, so the
transcript you see in skott is the same rendering `blad` uses to display
sessions afterwards.

## Prerequisites

**Building.** Rust **1.95** (MSRV) on Linux. `skott` is a workspace member, so
`cargo build -p skott` at the repository root builds it together with its
dependencies (`kvist-dependencies`, `sav`, `galla`, `blad`). No other tools are needed to build or
run the test suite.

**Running.** An interactive terminal and a TOML configuration (see
[Configuration](#configuration)). At least one reachable model provider — a
`llama-server` or `Ollama` endpoint — must be configured. For sandboxed work
(the default) a verified `galla-runner` and a Bubblewrap backend must be
installed; skott fails closed rather than falling back to host execution.
Rust builds additionally need a matching `Cargo.lock` and a vendored
`.kvist/vendored` registry provisioned on the host when external dependencies
are locked. Dependency-free Rust does not require a populated registry.

## Quick start

```
skott --init-config         # create a starter without overwriting existing files
skott                       # open the UI in the current directory
skott --config ./cfg.toml   # use a specific configuration
skott -m ollama "explain this repo"   # pick a model and submit a prompt
skott --list-models         # print configured models and exit
skott --setup               # shared discover/test/configure wizard; no config needed
skott --setup --registry ./providers.toml
skott --registry ./providers.toml -m local-coder
skott --vendor --cwd /path/to/project    # explicit host Rust vendoring; no config needed
skott --vendor --check                  # process-free, read-only metadata check
skott --vendor --offline                # reuse only existing Cargo cache
skott --vendor --offline --force        # explicit cached-only repair/rebuild
```

Interactive execution requires a terminal; use `--headless` for unattended
native workspace execution.

## Offline dependencies

From an ordinary Rust workspace with an existing `Cargo.lock`, run
`skott --vendor`, then `skott --vendor --check`, and start Skott normally.
If the lock is absent, resolve it explicitly on the host first. `--cwd`
selects the workspace; `--config` and model/run flags are deliberately
incompatible with this operation, so another project's config cannot redirect
dependency writes.

Vendoring is a human/operator host capability, **not a model tool or a
sandboxed install**. It may read host Cargo configuration and acquire the
exact locked packages. `--offline` restricts Cargo to its existing cache; it
does not promise OS network isolation of the host command. Toolchain/component
installation, lock resolution and sandbox networking are not enabled by this
command. Package-manager failure stops without automatic retries or fallback.

Kvist and Skott use the same headless `kvist-dependencies` library and existing
`.kvist/vendored`, `.kvist/sandbox-cargo/config.toml`, and
`.kvist/vendor.json` minimal receipt. Both load optional `.kvist/vendor.toml`
limits with shared defaults. The generated resolver accepts source
replacements only, including locked Git sources; your `.cargo/config.toml`
is not rewritten. Readiness checks package/native metadata presence and coherent
lock/resolver/receipt state, without hashing source files. Cargo retains and
checks its own required checksum metadata when consuming packages. Provisioning
stages privately, rejects conflicting
writers and lock drift, and preserves previous material before publication.
Interrupted publication or a remaining `.kvist/vendor-operation.lock`
requires inspecting the recorded state and reconciling it explicitly; never
blindly remove the lock while another operator may be active.

Agent execution stays network-denied and mounts the same shared tree read-only
at `/rust/vendor`, without copying packages; its writable workspace vendor alias remains
masked. **Restart the session after provisioning or dependency changes**
to refresh directory/lock identities and runtime config. Per-tool read locks
prevent publication while used, but idle sessions do not block it. Direct host
edits are outside that lock protocol. Use `--force` for damaged source; `--check`
does not attest full source integrity. See
[shared limits and rationale](../docs/sandbox-toolchains-and-vendoring.md#shared-configurable-limits).
Availability does not guarantee that arbitrary
build scripts, system libraries, or out-of-workspace path dependencies work.
The shared backend and Skott command support **Rust first**. Other languages
still need deliberately tested offline provisioning and sandbox resource
wiring; installing their system compiler alone is not dependency support.

The prompt is editable immediately while model/toolchain discovery and context
setup run in the background. You can submit before startup finishes; prompts
wait for validated tools and model readiness. Without `--model`, Skott reuses
the default provider's already-loaded model, including llama-server router
mode. A manual model choice is never overwritten by late discovery.

Startup shows a short toolchain list, Rust selection/vendor status, and model
context budget. Exact resource identities stay in private session logs rather
than cluttering the live transcript. **Ctrl+Q** restores the terminal first,
cancels initialization or active provider work, and then joins owned workers;
queued prompts do not start during shutdown.

## Prompt steering

When you submit a prompt while an autonomous task is already running, the
submission is treated as steering guidance for the active task rather than a
replacement task. The original objective, conversation context, completed
filesystem effects, and observed tool results are preserved. Steering is
delivered by appending a user message at the next safe interruption point:
quiet generation and retry waits are interrupted, but executing tools finish.
Provisional generations are discarded and stale tool calls receive explicit
not-executed results. Acceptance, claim and model-context delivery have distinct
statuses. Wall/token/attempt budgets are not reset. A locally submitted prompt
after completion is a new task; late remote guidance is rejected.

## Recovery and failures

Native prompts inspect answer/reasoning repetition and repeated tools, failed
observations and bounded action cycles. Long repeated passages are detected
without waiting for resource exhaustion. New pages and changed file revisions
remain usable; successful no-ops do not count as progress. Each prompt receives
at most two stall corrections, with one intervention per batch. Guidance and
compaction preserve the spent allowance, original goal and completed effects.
These are bounded heuristics, not proof of semantic progress.

Failures show a category, bounded diagnostics and a safe next step rather than
a success checkmark. Explicit limits identify the exhausted resource. When a
tool may have effects, inspect the current files/results before choosing another
action; cancellation and failed recording do not undo completed work. Restore
missing approved local tools/vendor resources separately, not by silently
downloading dependencies or expanding permissions.

Sandbox preparation uses the tool's wall-time budget, capped at 120 seconds
(the default), separately from execution. It scans the complete workspace,
including hidden and generated directories, on every call. A timeout names its
effective budget, active stage and total/stage elapsed time; it means the tool
did not execute, not that Git failed or the process ran out of memory.
For unusually large or slow workspaces, select a narrower `--cwd`. Opt-in
`SKOTT_LOG=skott::sandbox=debug` diagnostics report scan counts and preparation
durations without logging command text or output. Cancellation remains available
during preparation; no safety check is bypassed to make a timeout disappear.

For deliberately repetitive native output, use
`--disable-generation-loop-detection`. This explicit exception leaves tool
protection, budgets and sandbox authority intact. External agents own opaque
loops: native detection/steering are unsupported and possibly effectful
invocations are never automatically restarted.

## Headless mode

`skott --headless "..."` runs one sandboxed prompt without a terminal,
writing the durable session journal and transcript. `--json` emits version-one
NDJSON events rather than only the final answer. Headless execution is
sandbox-only: it requires durable logging and rejects `--allow-host-execution`,
`--host-turns`, and `--no-logs`. `kvist prompt` uses this same headless path
when no terminal is attached.

Headless model selection is deterministic: use `--model`, a sole configured
default-provider model, or that provider's configured default. It never changes
selection to whichever model happens to be loaded.

## Live inspection and optional controls

Native prompts print their private `*.events.jsonl` path. Protected
`kvist task run` attempts publish the same journal protocol:

```bash
skott --attach /path/to/run.events.jsonl
skott --attach /path/to/run.events.jsonl --follow  # plain-output follow
```

Attachment loads no providers or tools. At a terminal it opens a scrolling
viewer; otherwise it prints a bounded snapshot. EOF without `run_end` is
incomplete. `q`, Esc or Ctrl+C detaches the viewer without cancelling its owner.
These diagnostic records are not canonical Kvist evidence.

Start native execution with `--allow-control` to enable a private local socket.
For protected tasks use `kvist task run . TASK_ID --allow-control`. Its path is
reported only to the operator, never the model or durable journal:

```bash
skott --attach /path/to/run.events.jsonl --control /private/path/control.sock
skott --attach /path/to/run.events.jsonl --control /private/path/control.sock --guide "Preserve the public API"
skott --attach /path/to/run.events.jsonl --control /private/path/control.sock --cancel-run
```

With a capability, viewer `c` requests cancellation and `g` opens a guidance
composer (Enter queues, Esc discards). Queued does not mean delivered.
Cancellation does not undo effects; controls never grant new tools or
permissions. Skott IPC controls require private logs outside the workspace and
are incompatible with unconfined host execution.

## External coding agents

Shared external profiles, including Copilot, are listed by `--list-models`.
Select one explicitly:

```bash
skott --model copilot-coder --allow-host-execution "Review this workspace"
```

This terminal-only path requires an explicit **HOST UNCONFINED**
acknowledgement. The provider owns its loop; Sav supervises its literal command
once, without native tool interpretation or automatic retry. Private journals
retain bounded output. Viewer `c` cancels; `q` detaches while the owner keeps
waiting (Ctrl+C then requests cancellation). Native-only tool/context/token
overrides, headless external execution and protected task authoring are
refused. External live steering is unsupported rather than simulated.

## Configuration

Configuration is TOML. See [config.example.toml](./config.example.toml) for a
complete, commented example. Key points:

- `schema_version` must be `1`.
- Prefer `[registry] profiles = ["NAME"]` to reference Sav profiles without
  copying facts. Optional `path = "providers.toml"` is relative to this config;
  `--registry PATH` takes precedence. With no explicit path, project
  `.kvist/providers.toml` overrides the user Sav registry. Registry loading is
  filesystem-only; discovery/qualification happens only during explicit setup
  or execution startup. Native and external-agent backends remain distinct.
  Optional `default_model = "NAME"` binds the local fallback to one retained
  native shared profile of `default_provider`, without copying its backend.
- Explicit `models` overrides declare a local provider (`llama-server` or
  `ollama`), base URL, provider model name, and a per-turn deadline. Serving
  context is discovered for the selected model, not assumed to be 8192 tokens.
  Set per-model `context_limit` when the server cannot report its actual window
  (including an unloaded Ollama model). `response_reserve` controls the initial
  generation budget; by default it is up to 8192 tokens or a quarter of a small
  window. CLI budget overrides take precedence.
- `sandbox.runner` and `sandbox.backend` point at the `galla-runner`
  executable and the Bubblewrap backend; they default to resolved system paths.
- `tool_policy` exposes the shell denylist and the sandbox write root; the safe
  minimum denylist is always enforced.
- `[tool_profiles]` advertises language tool-chains only against what reaches the
  sandbox: each profile (`python`, `rust`, `javascript`, `go`, `c`) is `on`,
  `auto`, or `off`. `auto` advertises when the interpreter is available; `on`
  fails startup if it is not; `off` never advertises. The `Generic` base is
  always present.

### Using Skott outside Kvist

No Kvist project artifacts are required. Configuration discovery checks
`./skott.toml`, then `$XDG_CONFIG_HOME/skott/config.toml` (default
`$HOME/.config/skott/config.toml`), then system XDG locations. It does not search
ancestor directories. `--config PATH` overrides discovery; `--cwd DIR` selects
the execution workspace, not the configuration search directory.

If no configuration exists:

```bash
skott --init-config
skott --setup
```

Initialization is offline and creates only a private, no-clobber starter.
Setup is a separate, explicit discovery/qualification operation. Set the
starter's `default_provider` to match your configured native profiles, or edit
its commented `[[models]]` example. No model is invented by initialization.
Use `--init-config --config PATH` for another destination; its parent directory
must already exist. Initialization with `--cwd DIR` creates `DIR/skott.toml`
unless an explicit `--config` destination is given.

For quick reuse of an existing native profile, no full-config import is needed:

```bash
cd /path/to/another/project
skott --init-config --model qwen38-27b-q4-turbo \
  --registry /opt/proj/kvist/.kvist/providers.toml
skott
```

This writes only a reference to the selected profile, its absolute registry
source, and a matching fallback/provider selection. It copies no workspace,
sandbox or tool-policy settings, and does not modify the source registry.
Registry edits take effect on the next configuration load; moving/removing
that registry breaks the reference. For project-independent reuse, prefer
`skott --setup`'s user Sav registry at
`$XDG_CONFIG_HOME/sav/registry.toml` (default
`$HOME/.config/sav/registry.toml`), then
`skott --init-config --model NAME`. You can also create a user-wide Skott config
with `--config` at the discovery location above; omit `working_directory` so it
follows each launch directory. Project-local configs still take precedence.

**Reusing a whole config is allowed, but reuses all its settings.** In
particular, `/opt/proj/kvist/skott.toml` pins
`working_directory = "/opt/proj/kvist"`: launching it elsewhere still gives
the agent that workspace unless you pass `--cwd /path/to/another/project`.
Its registry path and custom themes resolve relative to the config file, not
the new workspace; configured runner/backend paths and tool policy also remain
in force. The config file's directory is not automatically writable, but an
explicitly configured workspace is. Prefer the selected-profile reference
above rather than copying another project's whole execution configuration.

Inside the sandbox the selected workspace appears at `/workspace`, regardless
of its host location. The whole selected directory is writable; Skott does not
apply Kvist task-level protected-path authority. Rust projects additionally
need host-provisioned locked/vendor resources; referencing provider profiles
does not provision them or authorize dependency downloads.

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
`skott/themes/dark.toml` and `light.toml` for the full, documented
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
highlighting are left untouched. Theme files are re-read from disk when a
theme is selected (the picker re-scans the `themes/` directory when it
opens), so editing a file — or dropping in a new one — applies immediately
with no restart.

Layout cues are theme-independent and keep content scannable: the transcript
and prompt panels carry a single top edge (no side or bottom borders), the
transcript always shows an adaptive right-edge scrollbar whose thumb encodes
the current window into the full transcript, model reasoning renders as
Markdown like the answer — lists, tables, and code fences keep their
structure — and is set apart by a left edge (`▌`/`│`) plus its tint, and
highlighted code keeps its indentation plus the theme's code patch. The
echoed prompt is distinct by its bold prompt foreground alone.

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
cargo build -p skott
cargo test -p skott
cargo clippy -p skott --all-targets
```

`skott` renders Markdown through the `blad` library: `blad` is the single
source of truth for Markdown parsing and rendering, and skott converts blad's
ANSI output to ratatui-compatible styled lines for the TUI. See
[`blad/README.md`](../blad/README.md) for the renderer itself.

The component follows the Kvist five-artifact model: [REQUIREMENTS.md](./REQUIREMENTS.md),
[CONTRACT.md](./CONTRACT.md), [DESIGN.md](./DESIGN.md), and [TODOS.yaml](./TODOS.yaml).
