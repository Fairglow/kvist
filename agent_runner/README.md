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
  `ollama`), base URL, provider model name, and a per-turn deadline.
- `sandbox.runner` and `sandbox.backend` point at the `kvist-sandbox-runner`
  executable and the Bubblewrap backend; they default to resolved system paths.
- `tool_policy` exposes the shell denylist and the sandbox write root; the safe
  minimum denylist is always enforced.

## Tools

The agent is offered a small, robust tool set that maps to sandbox-executed
commands: `shell`, `read_file`, `write_file`, and `list_dir`. Writes are confined
to the working directory (the sandbox write root); reading is lenient within the
sandbox view. The shell enforces a safe-by-default denylist over destructive
commands.

## Authority and safety

- No per-action prompts: authority is established once in configuration.
- Fail closed: a missing or unverified sandbox runner never triggers host
  execution.
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
