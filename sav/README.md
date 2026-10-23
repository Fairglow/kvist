# `sav`

`sav` is a standalone, bounded, Linux-first Rust library and CLI for the
provider-neutral mechanics of running AI agents and language models under
supervision. It owns prompt acquisition, shell-free command rendering, named
provider profiles, process supervision, local model transport, and the
provider-neutral native agent runtime. It is independent of Kvist
component discovery, task queues, roles, project policy, artifact promotion,
and compliance evidence; an embedding host supplies authority through narrow
interfaces.

`sav` is a workspace peer component. It is consumed by the `kvist` CLI
(`maerg`) and by `skott` as the `sav` Rust library, and it is also usable on
its own through the `sav-run` binary. It does not require the `kvist` CLI and
is not a sandbox: it supervises host processes and local model transports,
and it is the caller's responsibility to place it behind an enforcement
boundary when isolation is required.

## What it provides

- **Prompt acquisition** from a positional argument, a regular UTF-8 file,
  standard input, a terminal, or an editor.
- **Shell-free command rendering** with documented placeholders
  (`{prompt}`, `{prompt_json}`, `{context_files}`, `{target_directory}`); no
  shell is ever started.
- **Named provider profiles** that let multiple providers, models, and command
  templates coexist and be selected per prompt.
- **Process supervision** with bounded output, idle and loop detection,
  wall-clock and output limits, cancellation, and full process-group cleanup.
- **Local model transport** against loopback `llama-server` and `Ollama`, with
  streaming, usage accounting, loaded-model discovery, and bounded
  reasoning-text exposure.
- **Provider-model discovery** (`models`) against loopback HTTP catalogs and
  account-aware ACP catalogs, without running inference.
- **Shared native runtime** with host-injected tools, context fitting, retries,
  bounded steering and cancellation that preserve task accounting.
- **Independent observation** through private versioned live journals and
  separately authorized local control capabilities.

## Building

`sav` is a Rust 2024 package with MSRV Rust **1.95**. It builds as both a
library crate (`sav`) and a standalone CLI binary (`sav-run`):

```bash
# Build the library and the CLI
cargo build -p sav

# Release build of the CLI
cargo build --release -p sav

# Run the test suite
cargo test -p sav
```

The library is also built as a dependency of `maerg` and `skott`; building
the whole workspace (`cargo build --workspace`) covers it. No external
services are required to build or test `sav` itself; model transport and
provider discovery are exercised against loopback endpoints only when a
provider is actually configured.

## Running `sav-run`

`sav-run` is the standalone CLI. The commands below are the authoritative
interface; see the [User Guide](../docs/user-guide.md) for full
usage.

```bash
# Create a reusable provider profile interactively (probes, qualifies, saves)
sav-run setup

# List provider-advertised model IDs without running inference
sav-run models --provider ollama
sav-run models --provider llama-server --endpoint http://127.0.0.1:9931

# Run a supervised provider command from a prompt source
sav-run run --allow-host-execution --command "local-agent --prompt '{prompt}' {context_files}" --file prompt.md

# Inspect shared configuration without probing a provider
sav-run registry

# Resolve native and external profiles without copying provider facts
sav-run model --profile local-coder "Review this change"
sav-run run --allow-host-execution --profile copilot-coder "Review this change"
sav-run models --profile local-coder

# Send a text-only request through the local model transport
sav-run model --provider ollama --endpoint http://127.0.0.1:11434 --model qwen3-coder --stream --file prompt.md
sav-run model --provider llama-server --endpoint http://127.0.0.1:9931 --model Qwen3.8-9B-Q4_K_M "Summarize the supplied prompt"
```

`run` requires exactly one command template or stored profile and an explicit
`--allow-host-execution`; host mode is a reliability aid, not a sandbox.
`model` exposes no tools and performs no host process execution. Standalone
profiles are stored at `$XDG_CONFIG_HOME/sav/registry.toml`, falling back to
`$HOME/.config/sav/registry.toml`. `--registry PATH` overrides that source;
`.kvist/providers.toml` supplies project definitions. Whole named definitions
resolve in explicit > project > user order, without process probes or legacy
schema fallback.

```toml
schema_version = 1
[profiles.local-coder.backend]
kind = "native"
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "qwen3-coder"
context_limit = 32768

[profiles.copilot-coder.backend]
kind = "external-agent"
provider = "copilot"
executable = "copilot"
arguments = ["--prompt", "{prompt}", "--silent", "--model", "auto"]
model = "auto"
```

Native profiles cannot run as opaque processes, and external profiles cannot
be used as model transports. External argv is literal, not a parsed command.
Typed external profiles never restart automatically; positive process-retry
requests are rejected before execution.
Setup discloses inherited host authority and possible billing; `--force` can
persist failed qualification but does not authorize execution or claim tested
capabilities. Advertised catalog models and configured capability lists never
grant tool authority.

## Documentation

- [User Guide (Quick Start, Configuration & Usage)](../docs/user-guide.md)
- [Architecture](../docs/architecture.md)
- [Runtime Selection](../docs/runtime-selection.md)
- [Runner Hardening](../docs/runner-hardening.md)

The component's intent and realization live in
[`REQUIREMENTS.md`](REQUIREMENTS.md),
[`CONTRACT.md`](CONTRACT.md), and
[`DESIGN.md`](DESIGN.md). The layered runtime decision is documented in
[`docs/architecture.md`](../docs/architecture.md),
the current runtime choices and explicit limitations in
[`docs/runtime-selection.md`](../docs/runtime-selection.md). ADR 0009 records
the rejected Rig experiment; no Rig adapter is in the dependency graph.
The current host mode is a reliability aid, not a sandbox. `fakeroot`, retry
notices, and backups likewise do not restrict an agent's authority.
