# Sandbox Toolchains and Vendoring

This document explains how toolchains and their dependencies are mounted into
the sandbox, how they are discovered and used inside the sandbox, and how to
add or change vendored dependencies for each supported toolchain.

## Toolchain Mount Architecture

The `/rust/*` layout below belongs to **Skott workspace authoring**, not every
Galla request. **Maerg task verification** uses its separate closed Cargo
topology under `/workspace`, with enforced vendoring and approved identities.
**Maerg generic authoring** is a protected effect path: it neither queries rustup
nor stages a rustup home, and preserves its approved environment. Finding only
system rustup proxies in that path does not establish a usable Rust toolchain.

Kvist mounts toolchains into the sandbox in two different ways depending on
the toolchain:

### Rust: Explicitly Mounted Toolchain

The Rust toolchain is mounted as a discrete, identity-bound resource at a
fixed sandbox path. This allows Kvist to:

- Pin the exact compiler version used for builds
- Verify the toolchain hasn't drifted since provisioning
- Provide an offline, locked Cargo environment

The mount layout is:

| Sandbox Path              | Source                                                                                  | Purpose                                                                           |
| ------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `/rust/toolchain`         | Initially selected host toolchain directory | Read-only default compiler, standard library, and targets |
| `/rust/toolchains/N`      | Other validated installed toolchains, sorted by name | Read-only access to every other installed version and its targets |
| `/rust/runtime/bin/cargo` | Kvist-provided Cargo shim                                                               | A wrapper that forces `--offline --locked` and the vendored sources configuration |
| `/rust/vendor`            | Snapshot of `.kvist/vendored`                                                           | Read-only snapshot of the vendored dependency registry                            |
| `/rust/runtime/bin/rustc`, `/rust/runtime/bin/rustdoc` | Fixed selection-aware wrappers | Compiler and documentation selection follows Cargo or a leading `+name` |
| `/rust/rustup-home`       | Private generated settings and sandbox-native registrations for every installed name | Read-only complete installed inventory discovery |
| `/rust/user-cargo-bin`    | Validated optional host `~/.cargo/bin` directory | Read-only host-provisioned Cargo extensions; no host Cargo config or credentials |
| `/tmp/cargo-home`         | Private tmpfs                                                                           | Private, sandbox-local Cargo home (cache, registry metadata)                      |
| `/tmp/target`             | Private tmpfs                                                                           | Private, sandbox-local build target directory                                     |

The Cargo shim at `/rust/runtime/bin/cargo` is essential: it ensures that
normal PATH Cargo invocations default to offline/locked vendored resolution.
Explicit alternate Cargo paths are not rewritten; network isolation is enforced
independently by Galla, not by this convenience wrapper.

**PATH and environment variables:** The sandbox sets the following to make
the toolchain discoverable:

- `PATH` includes `/rust/runtime/bin:/rust/toolchain/bin` (before system paths)
- `RUSTC` is set to `/rust/runtime/bin/rustc`
- `RUSTDOC` is set to `/rust/runtime/bin/rustdoc`
- `CARGO_HOME` is set to `/tmp/cargo-home`
- `CARGO_TARGET_DIR` is set to `/tmp/target`
- `CARGO_NET_OFFLINE` is set to `true`
- `HOME=/tmp` names writable private invocation scratch
- `RUSTUP_HOME=/rust/rustup-home` contains generated settings, never host settings
- `RUSTUP_TOOLCHAIN` names the exact selected installed channel
- `RUSTUP_AUTO_INSTALL=0` disables automatic toolchain installation

Generated `toolchains/<name>` links target `/rust/toolchain` for the initial
selection and `/rust/toolchains/N` for other versions, never host paths.
`rustup toolchain list` and per-version installed-target/component queries see
the complete validated host inventory. Use `cargo +<name>`, `rustc +<name>`,
`rustdoc +<name>`, `rustup run <name> ...`, or invocation-local
`RUSTUP_TOOLCHAIN=<name>` to select another installed version. Compiler and
rustdoc wrappers follow Cargo selection; PATH Cargo retains offline/locked
vendored resolution. Explicit concrete Cargo paths remain unwrapped.
All toolchains and generated settings are read-only; installation is disabled.
Inventory/target drift requires restart after host provisioning. Invalid,
custom-linked or over-bound installations fail preparation rather than being
silently omitted. Host Cargo/rustup overrides are still ignored at startup.
Missing requested formatter/Clippy components fail before effects; provision
them separately on the host.

An agent running inside the sandbox can use Rust without knowing the mount
path — standard PATH discovery works: `which rustc`, `which cargo`, and
`which rustdoc` all resolve correctly.

### Other Toolchains: System PATH Discovery

Python, JavaScript/Node, Go, and C toolchains use a different approach. They
are expected to be installed in standard system locations on the host, and
the sandbox mounts those locations read-only. Discovery is via PATH, not
fixed mount paths.

The sandbox mounts the following host system directories read-only:

- `/usr`
- `/lib`
- `/lib64`
- `/bin`
- `/sbin`

Skott's prepared Rust profile also exposes the optional `~/.cargo/bin` directory
at `/rust/user-cargo-bin`, after the Cargo shim and concrete compiler in PATH.
The host home itself, Cargo credentials/configuration and rustup settings are
not mounted. System-profile detection alone does not authorize home-directory
mounts for other languages.

**PATH and environment variables:** The sandbox sets `PATH` to include the
sandbox-local directories first, then the system directories. For Rust, this
is `/rust/runtime/bin:/rust/toolchain/bin:/usr/bin:/bin:/usr/sbin:/sbin`.
For other toolchains, the system directories are sufficient.

## How Toolchains Are Found Inside the Sandbox

An agent running inside the sandbox discovers toolchains in the standard
Unix way:

1. **PATH-based discovery:** Commands like `cargo`, `rustc`, `python3`, `node`,
   `go`, and `gcc` are found via PATH.
2. **Environment variables:** For Rust, `RUSTC` and `RUSTDOC` are explicitly
   set. Other toolchains use PATH-only discovery.
3. **Version commands:** Agents can verify toolchain availability with
   `rustc --version`, `cargo --version`, `python3 --version`, `node --version`,
   `go version`, `gcc --version`, etc.

The tool definitions exposed to the agent document the available toolchains.
For example, the shell tool description includes: "Available tooling: coreutils,
git, and common Linux tools, cargo, rustc, and the Rust standard toolchain."

When the Rust toolchain is available, the tool description also includes:
"Rust uses an already installed read-only concrete toolchain; cargo is
explicitly offline with a private read-only vendor snapshot, HOME/cache/target
are scratch. Missing dependencies require host provisioning; sandbox builds
never download."

## Vendored Dependencies

Vendored dependencies are the package sources that sandbox builds resolve
against. They are provisioned on the host, outside the sandbox, and then
mounted into the sandbox read-only for offline builds.

### Rust (Cargo)

Rust vendoring is enforced exactly through `Cargo.lock`. The workflow is:

1. **Provision dependencies on the host:** Run `kvist vendor .` from the
   project root. This reconciles the vendored registry against `Cargo.lock`,
   downloading any missing crates into `.kvist/vendored/`. Skott does not
   provision dependencies or update locks while building. Maerg verification
   enforces its vendoring manifest through its own approved lifecycle.
2. **Build in the sandbox:** The sandbox mounts `.kvist/vendored` at
   `/rust/vendor` and uses the Cargo shim to force offline, locked builds
   against the vendored sources.

**To add or change dependencies:**

1. Edit `Cargo.toml` to add or change dependency specifications.
2. Run `cargo update` or `cargo add` on the host to update `Cargo.lock`.
3. Run `kvist vendor .` to synchronize the vendored registry.
4. Commit `Cargo.toml`, `Cargo.lock`, and the updated vendored registry.

The sandbox build will then use the updated dependencies. No network access
is needed inside the sandbox.

**Key constraints:**

- `Cargo.lock` must exist and be up-to-date. Sandboxed builds use `--locked`,
  which fails if `Cargo.lock` is stale or missing.
- The vendored registry must contain all dependencies listed in `Cargo.lock`.
  Run `kvist vendor .` to ensure this.
- Sandbox builds never download crates. If a dependency is missing from the
  vendored registry, the build fails.

### Agent dependency requests (`request_dependency`)

The Kvist agent runtime provides a `request_dependency` tool that agents can
use to request new or changed dependencies mid-task. When an agent calls this
tool:

1. The dependency origin is validated against the dependency policy (exact,
   pinned revision from a public source)
2. An in-policy request immediately triggers the dependency acquisition phase
3. The acquisition sandbox runs `cargo fetch` with allowlisted package sources
4. The fetched material is repacked into the vendored registry
5. The agent continues without interruption

Out-of-policy requests (unpinned, private, or untrusted origins) are surfaced
as decisions that pause the run for human review.

This means agents can autonomously request dependencies they need, without
requiring manual intervention to run `kvist vendor`.

### Python (uv)

Python vendoring uses `uv`'s lock files. The workflow is:

1. **Provision dependencies on the host:** Run `kvist vendor .` from the
   project root. This detects `uv.lock` (or `requirements.txt`), resolves
   the dependency tree, and packages the dependencies for offline use.
2. **Build in the sandbox:** The sandbox uses `uv`'s offline mode to install
   dependencies from the vendored package cache.

**To add or change dependencies:**

1. Edit `pyproject.toml` or `requirements.txt` to add or change dependency
   specifications.
2. Run `uv lock` or `uv sync` on the host to update `uv.lock`.
3. Run `kvist vendor .` to synchronize the vendored package cache.
4. Commit `pyproject.toml`, `uv.lock`, and the updated vendored cache.

### Node.js (npm/pnpm)

Node.js vendoring uses `package-lock.json` (npm) or `pnpm-lock.yaml` (pnpm).
The workflow is:

1. **Provision dependencies on the host:** Run `kvist vendor .` from the
   project root. This detects the lock file, resolves the dependency tree,
   and packages the dependencies for offline use.
2. **Build in the sandbox:** The sandbox uses the package manager's offline
   mode to install dependencies from the vendored cache.

**To add or change dependencies:**

1. Edit `package.json` to add or change dependency specifications.
2. Run `npm install` or `pnpm install` on the host to update the lock file.
3. Run `kvist vendor .` to synchronize the vendored cache.
4. Commit `package.json`, the lock file, and the updated vendored cache.

### Go

Go vendoring uses the module cache. The workflow is:

1. **Provision dependencies on the host:** Run `kvist vendor .` from the
   project root. This detects `go.mod` and `go.sum`, downloads the required
   modules, and packages them for offline use.
2. **Build in the sandbox:** The sandbox sets `GOPATH` to the vendored module
   cache and builds in offline mode.

**To add or change dependencies:**

1. Edit `go.mod` or run `go get` on the host to add or change dependencies.
2. Run `go mod tidy` to update `go.sum`.
3. Run `kvist vendor .` to synchronize the vendored module cache.
4. Commit `go.mod`, `go.sum`, and the updated vendored cache.

### C/C++ (Conan)

C/C++ vendoring uses Conan's lock files. The workflow is:

1. **Provision dependencies on the host:** Run `kvist vendor .` from the
   project root. This detects `conan.lock` (or `conanfile.txt`), resolves
   the dependency graph, and packages the dependencies for offline use.
2. **Build in the sandbox:** The sandbox uses Conan's offline mode to install
   dependencies from the vendored cache.

**To add or change dependencies:**

1. Edit `conanfile.txt` or `conanfile.py` to add or change dependencies.
2. Run `conan lock` on the host to update `conan.lock`.
3. Run `kvist vendor .` to synchronize the vendored cache.
4. Commit `conanfile.txt`/`conanfile.py`, `conan.lock`, and the updated
   vendored cache.

## Toolchain Provisioning

The Rust toolchain is provisioned separately from vendored dependencies. The
workflow is:

1. **Pin the toolchain (optional):** Add `rust-toolchain.toml` or
   `rust-toolchain` to the project root to specify the desired toolchain
   channel. Without a pin, the host's default rustup toolchain is used.
2. **Provision the toolchain:** Run `kvist toolchain .` from the project root.
   This resolves the pinned channel (or host default), ensures the toolchain
   is installed on the host, and records a manifest in `.kvist/rust-toolchain.json`.
3. **Verify the toolchain:** Run `kvist doctor` to verify the toolchain
   manifest is current and the toolchain is available.

**To upgrade or downgrade the toolchain:**

1. Update the `channel` field in `rust-toolchain.toml` (or edit the plain
   `rust-toolchain` file).
2. Run `kvist toolchain .` to install the new toolchain and update the manifest.
3. Run `kvist doctor` to verify the new toolchain is available.

## Inspecting Toolchain State

Use `kvist doctor` to inspect the project's toolchain state:

```
$ kvist doctor
...
[Rust Toolchain]
  channel: stable
  available: yes
...
```

If the toolchain manifest is missing or stale, the doctor will report:

```
[Rust Toolchain]
  channel: stable
  manifest: missing
  diagnostic: no toolchain manifest found; run `kvist toolchain` to provision
```

## Testing Toolchain Availability

Run the self-contained native chain with a runner built outside the worktree:

```bash
cargo build --locked -p galla --bin galla-runner
KVIST_RUST_TEST_RUNNER=/opt/target/debug/galla-runner \
  cargo test --locked -p skott --test rust_build_environment -- \
  --include-ignored --test-threads=1 --nocapture \
  --skip cargo_installed_tools_are_available_in_sandbox \
  --skip installed_minimal_pin_builds_without_optional_components \
  --skip native_repository_tests_execute_inside_the_workspace_sandbox
```

Replace `/opt/target` if using a different outside-worktree target directory.
The host must provide `/usr/bin/rustup`, an installed standard-layout toolchain,
its requested rustfmt/Clippy components, system cc/ar/as, and working Bubblewrap.
The CI quality job supplies these prerequisites and runs this chain explicitly.
No provider, network acquisition or pre-existing vendor tree is needed for the
self-contained fixture tests.

Run every maerg native toolchain trial with
`KVIST_GALLA=/opt/target/debug/galla-runner cargo test --locked -p kvist --test sandbox_toolchain -- --include-ignored --test-threads=1 --nocapture`.
Its generic verification trials explicitly grant concrete installed executables,
and the build trial compiles a real isolated locked fixture. The separate
`offline_cargo_verification_e2e` covers manifest-enforced vendoring and pinned
toolchains; do not conflate those authority paths.

For alternate installed channels, set `KVIST_RUST_TEST_CHANNEL=nightly` and run
`selected_toolchain_runs_the_complete_native_chain -- --ignored --nocapture`.
For a minimal installation without rustfmt, set
`KVIST_RUST_TEST_MINIMAL_CHANNEL=1.95.0` and run
`installed_minimal_pin_builds_without_optional_components`; this also verifies
that subsequently requesting the absent formatter fails actionably.
Run `native_repository_tests_execute_inside_the_workspace_sandbox` only after
provisioning this repository's `.kvist/vendored`; it compiles and executes the
actual Galla library tests inside the workspace sandbox. The Cargo-extension
trial additionally requires host-provisioned cargo-nextest. Explicit native
trials fail rather than silently claiming missing prerequisites.

Use `SKOTT_LOG=skott::toolchain::rust_environment=debug,skott::sandbox=trace skott ...`
for selection queries, preparation timings/counts/identities, validated grant
metadata and environment names, runner dispatch and observed exit/timeout/
cancellation/output-bound flags. Diagnostics go to stderr, not model-facing
stdout. They exclude command text, environment values and captured output.
`diagnostics_cover_the_chain_without_command_or_output_payloads` checks both
successful/nonzero completion and payload exclusion using a real runner.

Kvist includes tests that verify toolchain discoverability inside the sandbox:

- `skott/tests/rust_build_environment.rs` — Rust toolchain mounting, resolution,
  and offline build verification
- `skott/tests/toolchain_discoverability.rs` — Discoverability tests for all
  supported toolchains (Rust, Python, JavaScript, Go, C)
- `maerg/tests/language_offline_e2e.rs` — End-to-end offline build verification
  for all supported languages

These suites cover different consumer profiles and authority paths; version
probes alone do not establish functional tooling, and ignored or self-skipping
trials do not establish availability unless they actually execute.

`galla/tests/toolchain_e2e.rs` adds strict native system-language execution and
full installed-rustup inventory parity for explicit read-only grants, including
per-version target compilation and host non-mutation snapshots. CI explicitly
runs these otherwise ignored tests. `skott/tests/rustup_inventory.rs` additionally
drives actual automatic Skott preparation, compares every installed version and
target, verifies compiler/doc selection under all three Cargo selection forms,
and checks denied mutations and unchanged host installations. CI explicitly
runs it using the same separately provisioned multi-version/cross-target
prerequisites. Maerg's closed Cargo topology remains unchanged.
See [Galla toolchain testing review](galla-toolchain-testing-review.md) for
findings, limitations, recorded results and reproduction instructions.
