# Plan: Remove All `#[ignore]` Attributes from Tests

## Objective
Remove every `#[ignore]` attribute from all tests in the repository so that **all tests run by default** and pass without requiring network access or installation inside the sandbox.

## Findings

There are 27 `#[ignore]` attributes across 7 test files:

| File | Test Count |
|------|------------|
| `skott/tests/rust_build_environment.rs` | 11 |
| `skott/tests/live_llama.rs` | 3 |
| `skott/tests/toolchain_discoverability.rs` | 5 |
| `maerg/src/toolchain.rs` | 1 (unit test) |
| `maerg/tests/sandbox_toolchain.rs` | 5 |
| `maerg/tests/offline_cargo_verification_e2e.rs` | 2 |
| `galla/tests/toolchain_e2e.rs` | 2 |

## Sandbox Verification

Verified available in this sandbox:

| Tool | Available | Version |
|------|-----------|---------|
| `gcc` | ✅ | 16.2.1 |
| `g++` | ✅ | 16.2.1 |
| `cc` | ✅ | 16.2.1 |
| `clang` | ✅ | 23.1.1 |
| `go` | ✅ | 1.27.2 |
| `python3` | ✅ | 3.14.7 |
| `node` | ✅ | 20.20.2 |
| `npm` | ✅ | 12.2.0 |
| `java` | ✅ | 27 |
| `rustup` | ✅ | 1.29.1 |
| `make` | ✅ | 4.4.1 |
| `cmake` | ✅ | 4.4.4 |
| `bwrap` | ✅ | present |

**Rust toolchain issue:** `rustup toolchain list` returns empty. The sandbox has rustup but no installed Rust toolchains. This is the root cause for Rust-related test failures.

## Strategy

### Step 1: Fix Sandbox Rust Toolchain Setup
The sandbox must have Rust toolchains pre-installed. Options:
- Run `rustup default stable` during sandbox provisioning
- Or configure the sandbox to include a toolchain by default

### Step 2: Remove `#[ignore]` Attributes
Remove all 27 `#[ignore]` attributes from the identified test files.

### Step 3: Verify Tests Pass
Run the full test suite and verify all tests pass without network or installation.

### Step 4: Handle Failures
For any tests that fail due to network/installation requirements, either:
- Mock the network dependency
- Rewrite the test to work offline
- Remove the test if it fundamentally requires network/installation

## Risks
- Some tests may genuinely require network access (e.g., fetching crates)
- The sandbox Rust toolchain setup may need to be reconfigured
- Removing `#[ignore]` may cause CI failures if the sandbox environment differs

## Success Criteria
- All 27 `#[ignore]` attributes removed
- All tests pass without network access
- No test requires installation inside the sandbox
- The sandbox contains everything needed to run all tests
