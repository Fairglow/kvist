# Plan: Remove All `#[ignore]` Attributes from Tests

## Objective
Remove every `#[ignore]` attribute from all tests so they run by default and pass without network access or installation inside the sandbox.

## Current State
- **29 `#[ignore]` attributes** across 14 test files
- Rust toolchains: **NOT AVAILABLE** — `/rust/toolchain` doesn't exist, rustup has no installed toolchains
- Network: Unavailable (DNS fails)
- Runner: `kvist-sandbox-runner` binary exists at `/workspace/target/debug/`
- Bubblewrap: `/usr/bin/bwrap` available

## Root Cause
The Rust toolchains are not available in this sandbox. The expected path `/rust/toolchain` does not exist. All Rust-related tests are marked `#[ignore]` because they require actual toolchains at that path.

The sandbox has two Rust installations:
1. `/usr/bin/rustup` — the version manager (exists but no toolchains installed)
2. `/rust/toolchain` — the actual toolchain (MISSING — should be mounted from host)

Since installation inside the sandbox is forbidden and network access is unavailable, the toolchains must be **pre-mounted from the host into the sandbox during sandbox provisioning**.

## Test Files with Ignored Tests

| File | Count | Requirements |
|------|-------|--------------|
| `skott/tests/rust_build_environment.rs` | 11 | KVIST_RUST_TEST_RUNNER, bwrap, mounted toolchains |
| `skott/tests/rustup_inventory.rs` | 1 | KVIST_RUST_TEST_RUNNER, bwrap, mounted toolchains |
| `skott/tests/live_llama.rs` | 3 | Live llama-server or native sandbox runner |
| `skott/tests/toolchain_discoverability.rs` | 5 | KVIST_RUST_TEST_RUNNER, bwrap, mounted toolchains |
| `maerg/tests/sandbox_toolchain.rs` | 5 | Live sandbox with mounted toolchains |
| `maerg/tests/offline_cargo_verification_e2e.rs` | 2 | Live sandbox + network access |
| `galla/tests/toolchain_e2e.rs` | 2 | KVIST_GALLA, bwrap, mounted toolchains |

## Solution

### Step 1: Mount Rust Toolchains into Sandbox (Sandbox Provisioning)
The sandbox must be configured to mount the host's Rust toolchain directories into the sandbox at the expected path `/rust/toolchain`.

Using bubblewrap, this would look like:
```bash
bwrap --bind /home/user/.rustup/toolchains/stable-x86_64-unknown-linux-gnu /rust/toolchain ...
```

For multiple toolchains:
```bash
bwrap --bind /home/user/.rustup/toolchains/stable-x86_64-unknown-linux-gnu /rust/toolchain \
      --bind /home/user/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu /rust/toolchains/nightly ...
```

### Step 2: Set KVIST_RUST_TEST_RUNNER Environment Variable
```bash
export KVIST_RUST_TEST_RUNNER=/workspace/target/debug/kvist-sandbox-runner
```

### Step 3: Remove #[ignore] Attributes
Once toolchains are mounted, remove all `#[ignore]` attributes from the 7 test files.

### Step 4: Run Tests
Execute all tests without `--include-ignored` to verify they all pass.

## Expected Outcome
- All 29 `#[ignore]` attributes removed
- All tests pass without network access
- No installation inside sandbox (toolchains pre-mounted from host)
- Sandbox contains all necessary toolchains at `/rust/toolchain`