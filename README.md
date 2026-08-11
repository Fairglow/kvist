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

## Root artifact templates

`kvist init` will create the following deterministic, UTF-8 templates. Writing
them to disk is intentionally deferred until its implementation is complete.

| Path | Version and required defaults | Purpose |
| --- | --- | --- |
| `kvist.toml` | `schema_version = 1`; `component_root = "src"`; `llm.provider = "none"` | Project-local configuration with opt-in external LLM integration. |
| `ROOT_CONTRACT.md` | `kvist-template-version: 1` | Global architectural and compliance constraints for every component. |
| `src/SPEC.md` | `kvist-template-version: 1` | Root component contract with the three progressive-disclosure layers. |
| `src/TODOS.yaml` | `schema_version: 1` | Ordered lifecycle tasks: tests, implementation, security audit, compliance review. |
| `src/DOCS.md` | `kvist-template-version: 1` | Independently reverse-engineered implementation documentation. |

Template and schema versions are positive integers. Backward-incompatible
changes must increment the relevant version and include an explicit migration
path; Kvist must never silently rewrite user-authored artifacts. The initial
templates contain no credentials, configured external provider, copyright
notices, or license terms.

## Dependencies

The CLI uses [clap](https://crates.io/crates/clap) 4 for typed, accessible
argument parsing and help generation, and
[thiserror](https://crates.io/crates/thiserror) 2 for concise, typed domain
errors. Both are mature, widely maintained Rust ecosystem dependencies. The
project keeps its dependency graph small and adds dependencies only when their
security, licensing, maintenance, and operational benefits are justified.

Tests use [toml](https://crates.io/crates/toml) 1 to verify that the
configuration template is valid TOML; it is a development-only dependency.
