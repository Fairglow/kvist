# Agent Environment Guide

This is the Kvist workspace. When executing tasks, be aware of these environment characteristics:

## Rust Build Environment

- **Target directory**: `/workspace/target` — persistent, user-expected location
- **Vendor directory**: `/rust/vendor` — project vendoring location, mounted read-only
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

- Builds are always offline; dependencies come from the vendored snapshot at `/rust/vendor`
- The target directory is persistent across sandbox sessions
- No network access is available in the sandbox
- `cargo` and `rustc` are wrapper scripts that inject offline/vendor flags automatically

## Toolchain Discovery

```bash
# Check active toolchain
rustup show active-toolchain

# List available toolchains
rustup toolchain list
```

## Environment Variables

The following environment variables may be relevant to agent tasks:

- `KVIST_CARGO_WORKDIR` — if set, the `cargo` wrapper changes to this directory first
- `KVIST_COMPONENT_BUILD_DIR` — used for component-build path remapping in the `rustc` wrapper

## Quick Environment Probe

```bash
echo "Rust: $(rustc --version 2>/dev/null)"
echo "Active toolchain: $(rustup show active-toolchain 2>/dev/null)"
echo "Vendor dir exists: $(test -d /rust/vendor && echo yes || echo no)"
echo "Target dir: $CARGO_TARGET_DIR"
```
