# Sandbox Toolchains and Vendoring

This document explains how toolchains and their dependencies are mounted into
the sandbox, how they are discovered and used inside the sandbox, and how to
add or change vendored dependencies for each supported toolchain.

## Toolchain Mount Architecture

Kvist mounts toolchains into the sandbox in two different ways depending on
the toolchain:

### Rust: Explicitly Mounted Toolchain

The Rust toolchain is mounted as a discrete, identity-bound resource at a
fixed sandbox path. This allows Kvist to:

- Pin the exact compiler version used for builds
- Verify the toolchain hasn't drifted since provisioning
- Provide an offline, locked Cargo environment

The mount layout is:

| Sandbox Path | Source | Purpose |
|---|---|---|
| `/rust/toolchain` | Host toolchain directory (e.g., `~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu`) | Read-only access to the Rust compiler, standard library, and targets |
| `/rust/runtime/bin/cargo` | Kvist-provided Cargo shim | A wrapper that forces `--offline --locked` and the vendored sources configuration |
| `/rust/vendor` | Snapshot of `.kvist/vendored` | Read-only snapshot of the vendored dependency registry |
| `/tmp/cargo-home` | Private tmpfs | Private, sandbox-local Cargo home (cache, registry metadata) |
| `/tmp/target` | Private tmpfs | Private, sandbox-local build target directory |

The Cargo shim at `/rust/runtime/bin/cargo` is essential: it ensures that
every Cargo invocation inside the sandbox uses offline mode with the vendored
sources, regardless of what the agent or build scripts request.

**PATH and environment variables:** The sandbox sets the following to make
the toolchain discoverable:

- `PATH` includes `/rust/runtime/bin:/rust/toolchain/bin` (before system paths)
- `RUSTC` is set to `/rust/toolchain/bin/rustc`
- `RUSTDOC` is set to `/rust/toolchain/bin/rustdoc`
- `CARGO_HOME` is set to `/tmp/cargo-home`
- `CARGO_TARGET_DIR` is set to `/tmp/target`
- `CARGO_NET_OFFLINE` is set to `true`

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

Additionally, for toolchains that live in user home directories, Kvist
mounts those locations when the corresponding profile is enabled:

| Toolchain | User Home Directories Mounted |
|---|---|
| Rust | `~/.cargo/bin` |
| Go | `~/go/bin`, `~/.local/bin` |

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
   downloading any missing crates into `.kvist/vendored/`.
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

Kvist includes tests that verify toolchain discoverability inside the sandbox:

- `skott/tests/rust_build_environment.rs` — Rust toolchain mounting, resolution,
  and offline build verification
- `skott/tests/toolchain_discoverability.rs` — Discoverability tests for all
  supported toolchains (Rust, Python, JavaScript, Go, C)
- `maerg/tests/language_offline_e2e.rs` — End-to-end offline build verification
  for all supported languages

These tests ensure that toolchains are available, discoverable, and functional
inside the sandbox before any agent uses them.
