# Kvist

Kvist is a Rust CLI for a filesystem-native, spec-driven architecture workflow
for human-directed AI development. Its product architecture is defined in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

## Initial CLI contract

| Command | Contract |
| --- | --- |
| `kvist init [PROJECT_DIR]` | Initialize the Kvist root artifacts in `PROJECT_DIR`, defaulting to the current directory. |
| `kvist tree [PROJECT_DIR]` | Render the component hierarchy rooted at `PROJECT_DIR`, defaulting to the current directory. |
| `kvist spec new <COMPONENT_DIR>` | Create a layered `SPEC.md` for a component directory. |
| `kvist spec validate <SPEC_FILE>` | Validate a layered `SPEC.md` file. |

The commands are defined and provide help output now. Their filesystem behavior
is implemented by later Phase 1 tasks and currently returns an explicit
unavailable-command error.

## Configuration and platform policy

The initial configuration is project-local only: `kvist.toml` lives directly in
the selected project root. Kvist does not read global configuration and does
not search parent directories for a project. Relative command paths are
resolved by the operating system from the current working directory.

Kvist targets current stable Rust on Linux, macOS, and Windows for x86_64 and
ARM64 systems. Filesystem behavior must be covered on every supported platform;
platform-specific differences must be explicit in the relevant command
documentation and tests.

## Dependencies

The CLI uses [clap](https://crates.io/crates/clap) 4 for typed, accessible
argument parsing and help generation, and
[thiserror](https://crates.io/crates/thiserror) 2 for concise, typed domain
errors. Both are mature, widely maintained Rust ecosystem dependencies. The
project keeps its dependency graph small and adds dependencies only when their
security, licensing, maintenance, and operational benefits are justified.
