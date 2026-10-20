# maerg — the `kvist` CLI and workflow engine

`maerg` is the root component of Kvist. It provides the local `kvist`
executable and the headless workflow engine: project initialization,
component-document validation, bounded discovery, project status, durable task
queues, the task lifecycle, Kvist-specific role and policy selection, sandbox
integration, context selection, import/conversion, and durable execution
evidence.

`maerg` owns the project and component artifacts and the CLI dispatch. It does
not own model-provider wire protocols, generic command supervision, or reusable
provider profiles — those belong to the `sav` component, which `maerg` consumes
as a Rust library. It speaks to the `galla` enforcement boundary only through
the versioned sandbox protocol; it never imports `galla` types and never
executes an agent or verification command directly.

The full command table and workflow design live in
[`docs/command-set.md`](../docs/command-set.md); the root project README
([`README.md`](../README.md)) documents the CLI contract, configuration,
sandboxed task execution, and language/toolchain support. The component's
intent and realization are in [`REQUIREMENTS.md`](REQUIREMENTS.md),
[`CONTRACT.md`](CONTRACT.md), and [`DESIGN.md`](DESIGN.md).

## Building

`maerg` is a Rust 2024 package with MSRV Rust **1.95**. It builds the `kvist`
binary (the package name is `kvist`):

```bash
# Build the CLI
cargo build -p kvist

# Release build
cargo build --release -p kvist

# Run the test suite
cargo test -p kvist
```

The package is the workspace default member, so a plain `cargo build` at the
repository root builds `kvist`. No external services are required to build or
test the read-only workflow commands; the supervised `task run` and `prompt`
paths additionally require a separately installed `galla-runner` and a
Bubblewrap backend (see the root README's sandboxed task execution section).

## Running

`kvist` resolves its context from the working directory: the project root is
found by walking upward to the nearest `kvist.toml` (like `git`), and component
commands act on the component containing the current directory unless a
`COMPONENT_DIR` argument is given. Bare `kvist` (no arguments) shows the project
overview inside a project and guided help outside one.

```bash
kvist init                 # create the root artifacts in the current directory
kvist status               # "where do I stand?" — states, progress, next action
kvist tree                 # render the component hierarchy
kvist doctor               # read-only inspection and recovery guidance
kvist component new <dir>  # create REQUIREMENTS/CONTRACT/DESIGN templates
kvist task next            # select the first ready task
kvist prompt "..."         # run a prompt in the skott shell
kvist help                 # guided tour and topics
```

The complete, authoritative command set is documented in
[`docs/command-set.md`](../docs/command-set.md).
