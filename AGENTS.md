# Agent Environment Guide

This is the Kvist workspace. When executing tasks, be aware of these environment characteristics:

## Rust Build Environment

- **Target directory**: `/workspace/target` — persistent, user-expected location
- **Vendor directory**: `/workspace/.kvist/vendored` — project vendoring location, mounted via workspace grant
- **Offline builds**: `cargo` always runs with `--offline --locked` against the vendored sources
- **Toolchain selection**: Use `rustup show active-toolchain` to discover the active toolchain
- **Toolchain locations**: The default toolchain is at `/rust/toolchain`. Additional toolchains are under `/rust/toolchains/` (note the singular vs. plural naming).

## Build Workflow

```bash
# Standard build
cargo build --release

# Run tests
cargo test

# The build artifacts will be in /workspace/target/release/
```

## Important Notes

- Builds are always offline; dependencies come from the vendored snapshot
- The target directory is persistent across sandbox sessions
- Use the project's `.kvist/vendored/` directory as the source of truth for dependencies
- No network access is available in the sandbox

## Toolchain Discovery

```bash
# Check active toolchain
rustup show active-toolchain

# List available toolchains
rustup toolchain list
```
