# Sandbox Toolchains and Vendoring

This document explains how toolchains and their dependencies are mounted into
the sandbox, how they are discovered and used inside the sandbox, and how to
add or change vendored dependencies for each supported toolchain.

## Standalone Rust provisioning

`skott --vendor [--cwd DIR]` provisions an ordinary Rust project with an existing
`Cargo.lock`, without `kvist.toml`, `skott.toml`, model setup, or a UI.
`skott --vendor --check` performs acquisition-free readiness checks: canonical
graphs use filesystem metadata; Git/alternate graphs disclose bounded offline
host Cargo admission, which may write private administration or extract cached
material.
`skott --vendor --offline` uses Cargo's existing host cache only.
Both Skott and `kvist vendor` share `kvist-dependencies` and the selected native
Cargo cache: absolute `CARGO_HOME`, otherwise `HOME/.cargo`. Already downloaded
registry/Git data is reused directly; default preparation does not run
`cargo vendor`, duplicate dependencies, or scan every source file.
`--force` refreshes the small native receipt; it does not delete complete cached
packages, create a second dependency tree or guarantee corruption repair.
Neither command changes the lockfile.

Acquisition is explicit operator host authority; normal agent execution stays
strictly network-denied. Host Cargo may read operator configuration and contact
package sources unless `--offline` is selected. Cargo's offline flag is not OS
confinement. Configured Kvist acquisition errors never authorize a host
fallback; protected model dependency fulfillment remains unsupported.
Configured acquisition stages bounded exact manifests/locks and empty target
placeholders, not implementation content or host Cargo configuration. Git
declarations and lock sources must match approved HTTPS/full-revision pairs;
native Git/SSH transports and branch/tag-only declarations fail before fetch.
Other-language maerg strategies remain in place, but Skott's consolidated
offline backend is Rust-only until each language's resource wiring is tested.

Only native registry/Git data is exposed, not the operator's whole Cargo home,
credentials or configuration. Sandboxes use private writable Cargo administrative
state and read-only native dependency data. Native acquisition populates those
data roots incrementally; failed fetches retain completed downloads for another
explicit attempt. Project/lock identity is rechecked before a small atomic
receipt is published. Legacy portable-export locks/stages are separate and are
not deleted or stolen by native preparation.

### Minimal state and native Cargo validation

`.kvist/cargo-cache.json` is small, schema-versioned and lock-bound. It contains
no per-file inventory or content-checksum manifest and never grants authority to
arbitrary host paths. Readiness checks native package/index metadata; Cargo owns
native resolution and archive checksums. We do not manufacture directory-source
`.cargo-checksum.json` files or pretend a native cache is a Cargo vendor directory.
A cheap check is not a source-integrity attestation or a successful build.
Git/alternate sources may require bounded real offline/locked Cargo admission.

The data remains a live cache, not an immutable snapshot. Native locks coordinate
participating preparation; they cannot make unrelated host writers disappear.
External deletion/corruption yields an actual offline error, never a network
fallback. Explicit preparation acquires known missing data; native package-manager
repair may be needed for corruption that presence checks cannot detect. Default reuse should
complete below 30 seconds even for large warm graphs; necessary downloads,
extraction, package installation and builds are not universally time-bounded by
that target. See [ADR 0020](decisions/0020-native-offline-dependency-caches.md).

`kvist vendor --vendored-dir PATH` explicitly creates a portable Cargo directory
source with Cargo-generated checksums and resolver configuration. That deliberate
export retains its publication/recovery protocol; it is not the default cache
backend and does not redirect Skott's native resources.

### Shared configurable limits

Both applications load optional `.kvist/vendor.toml` independently of model
configuration. Use a single `[limits]` table; omissions use defaults. Unknown
keys, wrong types and nonpositive values fail explicitly. For example:

```toml
[limits]
max_total_bytes = 2147483648
timeout_secs = 1200
```

| Setting | Default |
| --- | ---: |
| `max_packages` | 100000 |
| `max_sources` | 1024 |
| `max_entries` | 100000 |
| `max_total_bytes` | 1073741824 (1 GiB) |
| `max_file_bytes` | 268435456 (256 MiB) |
| `max_path_bytes` | 33554432 (32 MiB) |
| `max_depth` | 64 |
| `max_lockfile_bytes` | 16777216 (16 MiB) |
| `max_metadata_bytes` | 16777216 (16 MiB) |
| `max_output_bytes` | 1048576 (1 MiB) |
| `timeout_secs` | 900 |

Native reuse inspects the locked catalogue and necessary package/index markers,
not a recursive inventory of dependency source trees. Portable directory export
and its explicitly selected cleanup subtrees retain bounded tree quotas.
The policy parser has a separate 1 MiB input boundary. Filesystem path syntax,
installed-toolchain inventory and sandbox protocol bounds are separate intrinsic
restrictions, not vendor quotas.

The repository ignores generated vendor/receipt/recovery/runtime state while
allowing the optional root `.kvist/vendor.toml` policy to be version controlled.
Retired receipt formats are not recognized; there is no implicit migration.

## Toolchain Mount Architecture

The `/rust/*` layout below belongs to **Skott workspace authoring** and
**maerg Rust authoring**, not every
Galla request. **Maerg task verification** uses its separate closed Cargo
topology under `/workspace`, with enforced offline sources and approved identities.
**Maerg non-Rust authoring** preserves its approved environment and requires no
Rust installation. For components with a local Cargo manifest, its protected
authoring path prepares the installed Rust inventory without granting write
access to protected component files or peer implementations.

Kvist mounts toolchains into the sandbox in two different ways depending on
the toolchain:

### Rust: Explicitly Mounted Toolchain

The Rust toolchain is mounted as a discrete, identity-bound resource at a
fixed sandbox path. This allows Kvist to:

- Pin the exact compiler version used for builds
- Provide live read-only installed compiler resources without a content freeze
- Provide offline Cargo (workspace locks may update; protected verification retains locked evidence rules)

The mount layout is:

| Sandbox Path                                           | Source                                                                               | Purpose                                                                           |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| `/rust/toolchain`                                      | Initially selected host toolchain directory                                          | Read-only default compiler, standard library, and targets                         |
| `/rust/toolchains/N`                                   | Other validated installed toolchains, sorted by name                                 | Read-only access to every other installed version and its targets                 |
| `/rust/runtime/bin/cargo`                              | Kvist-provided Cargo shim                                                            | Workspace `--offline` with native data; protected wrapper additionally forces `--locked` |
| `/rust/cargo-registry`                                 | Selected native Cargo registry data                                                  | Read-only registry cache/index/expanded sources, without copies                   |
| `/rust/cargo-git`                                      | Selected native Cargo Git data, when present                                         | Read-only native repositories/checkouts, without copies                           |
| `/rust/runtime/bin/rustc`, `/rust/runtime/bin/rustdoc` | Fixed selection-aware wrappers                                                       | Compiler and documentation selection follows Cargo or a leading `+name`           |
| `/rust/rustup-home`                                    | Private generated settings and sandbox-native selected/optional registrations | Read-only inventory discovery; omitted optional names have selection guidance |
| `/tmp/rustup-home`                                     | Settings copied inside the sandbox; link to the read-only registration directory     | Invocation-local selection/profile state; never host settings                     |
| `/rust/user-cargo-bin`                                 | Validated optional host `~/.cargo/bin` directory                                     | Read-only host-provisioned Cargo extensions; no host Cargo config or credentials  |
| `/rust/rustup`                                         | Trusted Skott system or host-home Rustup executable                                  | Live read-only file, without mounting host HOME; protected wrapper keeps its system path |
| `/tmp/cargo-home`                                      | Private tmpfs                                                                        | Private administrative home; data links target approved read-only native roots    |
| `/workspace/target`                                    | Writable Skott workspace                                                             | Persistent workspace build target directory                                      |

The Cargo shim at `/rust/runtime/bin/cargo` is essential: it ensures that
normal PATH Cargo invocations default to offline native resolution; Skott
does not force `--locked`. Protected consumers retain locked wrappers.
Explicit alternate Cargo paths are not rewritten; network isolation is enforced
independently by Galla, not by this convenience wrapper.

**PATH and environment variables:** The sandbox sets the following to make
the toolchain discoverable:

- `PATH` includes `/rust/runtime/bin:/rust/toolchain/bin` (before system paths)
  and retains all bin-only System directories, including `/usr/local/bin`
- `RUSTC` is set to `/rust/runtime/bin/rustc`
- `RUSTDOC` is set to `/rust/runtime/bin/rustdoc`
- `CARGO_HOME` is set to `/tmp/cargo-home`
- Skott sets `CARGO_TARGET_DIR=/workspace/target` so build artifacts persist
  across shell invocations; protected maerg authoring uses `/tmp/target` scratch
  instead, without granting writes outside the component's approved source roots
- `CARGO_NET_OFFLINE` is set to `true`
- `HOME=/tmp` names writable private invocation scratch
- `RUSTUP_HOME=/tmp/rustup-home` contains private settings, never host settings
- `RUSTUP_TOOLCHAIN` names the exact selected installed channel only for a pin
- `RUSTUP_AUTO_INSTALL=0` disables automatic toolchain installation

Generated `toolchains/<name>` links target `/rust/toolchain` for the initial
selection and `/rust/toolchains/N` for other versions, never host paths.
`rustup toolchain list` and per-version installed-target/component queries see
the complete validated host inventory. Use `cargo +<name>`, `rustc +<name>`,
`rustdoc +<name>`, `rustup run <name> ...`, or invocation-local
`RUSTUP_TOOLCHAIN=<name>` to select another installed version. Without a pin,
`rustup default <installed-name>` changes the default for the current shell
invocation; `rustup default stable` succeeds offline when stable is installed.
Each new sandbox invocation starts from the resolved host default or pin.
Compiler and
rustdoc wrappers follow Cargo selection; Skott PATH Cargo retains offline
native resolution. Explicit concrete Cargo paths remain unwrapped.
All toolchains and registration templates are read-only; private selection
settings are writable in tmpfs. Installation is disabled.
Skott refreshes relevant topology/configuration at subsequent tool boundaries,
including recovery from initial unavailability. Canonical trusted custom-linked
registrations are usable; optional failures are diagnosed without disabling the
selected compiler. A bounded plan retains that compiler and up to 127 other
registrations; pin an omitted name to select it. Host Cargo/rustup overrides
are still ignored. Missing optional formatter/Clippy resources fail only their
own commands; provision them on the host and retry without restart.

Maerg uses the same Sav runtime scripts but mounts all installations under
`/rust/toolchains/N` (without Skott's initial `/rust/toolchain` alias or optional
user Cargo-bin mount). It exposes local Cargo metadata read-only and uses selected native registry/Git roots when present. A local-only graph
needs no fabricated empty vendor tree. Under the
explicit approval recorded in ADR-0014, it automatically discovers declared
workspace members and local path dependencies and exposes their build resources
at `/rust/project`, preserving original Cargo manifests, workspace inheritance
and project-relative paths. Provider intent documents, hidden state and build
output/cache directories are excluded recursively. Synthesized ancestors are
sandbox-local directories, not writable host resources.

PATH Cargo runs from the component's build-view directory. Compiler, rustdoc
and formatter wrappers redirect component `src`/`tests` paths to the writable
authoring view; those local roots remain live for Cargo target discovery and
source edits. Provider files and root Cargo metadata remain read-only; providers
nested inside writable component roots are rejected rather than retaining a
writable alias. Discovery is bounded to 128 crates, 100000 source entries,
32 MiB of path state, depth 128 and 30 seconds, with the runner's 256-grant
limit applying to the final resource plan. Workspace-member patterns support
literal paths and whole-directory `*` segments, not recursive/extended globs.
Links and out-of-project resources fail explicitly. This build context is not
implicit prompt context and never grants peer writes. The closed Cargo
verification topology is unchanged.

Both Rust authoring consumers use `/rust/cargo-registry` and `/rust/cargo-git`
read-only with private administrative state. Protected provisioning/approval
checks remain separate from ordinary workspace authoring. No hidden mode-000
freeze or default `.kvist/vendored` alias is required; host cache bytes and modes
are unchanged. An absent data root causes no authoring-side host creation.

An agent running inside the sandbox can use Rust without knowing the mount
path — standard PATH discovery works: `which rustc`, `which cargo`, and
`which rustdoc` all resolve correctly.

### Other Toolchains: System PATH Discovery

Python, JavaScript/Node, Go, and C toolchains use a different approach. They
are expected to be installed in standard system locations on the host, and
the sandbox mounts those locations read-only. Discovery is via PATH, not
fixed mount paths.

The sandbox exposes the following System resources read-only:

- `/usr`
- `/lib`
- `/lib64`
- `/bin`
- `/sbin` (and the sbin directories under `/usr`)

Standard system bin/sbin trees are live read-only views; host links and
permissions are preserved. Names do not grant privileges: namespace/capability,
network and approved filesystem authority protect the host. Authoring PATH and
explicit runtime/Git/language environment values may select sandbox-local code,
including writable workspace scripts. Host enforcers run with cleared
environments; closed Cargo-phase source/environment rules remain separate.

Skott's prepared Rust profile also exposes the optional `~/.cargo/bin` directory
at `/rust/user-cargo-bin`, after the Cargo shim and concrete compiler in PATH.
The host home itself, Cargo credentials/configuration and rustup settings are
not mounted. System-profile detection alone does not authorize home-directory
mounts for other languages.

### Diagnosing Skott request preflight

Skott's request-preflight guard uses the requested tool wall time capped at
120 seconds, separate from the 180-second Rust startup
preparation, a sandbox command's execution timeout, and nextest's test timeout.
A longer test can still fail specifically during finite preflight.
Timeout errors include `limit_ms`, `stage`, `elapsed_ms` and
`stage_elapsed_ms`; `SKOTT_LOG=debug`
also records identity, workspace, Rust-resource, context and request-validation
stages. Logs omit command text, environment values and captured output.

There is no recursive writable-checkout inventory or frozen-resource scan.
Explicit grant origins are opened without following source/ancestor links and
handed off as close-on-exec, descriptor-backed mounts; the backend consumes raw
source capabilities before payload execution. Mounted bytes remain live;
atomic node replacements are selected at the next tool boundary. External CPU/storage contention
can still exhaust a finite deadline, and kernel calls cannot be preempted.
Run native repository tests through the automatically discovered
`.config/nextest.toml` policy so other tests in that invocation cannot mutate the
checkout concurrently with its whole-repository trial.

Both Rust Quality and MSRV CI jobs explicitly install Python, Node and C/C++
as system packages before native tests. A tool available only through the
runner's host toolcache is not necessarily available in the sandbox.
Native GCC discovery tests compare the sandbox version line with the queried
host system compiler, accepting distribution-specific branding without
accepting missing or mismatched version evidence.

The Galla executable trial separately logs ambient host candidates (including
unmounted home/toolcache tools), selects against the System namespace's bin
search order, checks canonical host/sandbox executable parity, inspects runtime
roots, and runs real Python, Node, Go, C and C++ programs. It does not grant
arbitrary host PATH directories. A Go binary is insufficient if its `GOROOT`
is outside the mounted System tree; provision its complete runtime on the host.

**PATH and environment variables:** The sandbox sets `PATH` to include the
sandbox-local directories first, then the system directories. For Rust, the core order
is `/rust/runtime/bin:/rust/toolchain/bin:/usr/bin:/bin:/usr/local/bin`.
For other toolchains, the system directories are sufficient.
Prepared Rust retains its remaining bin search precedence and includes
`/usr/local/bin`, so enabling Rust does not hide Node or other
already-mounted local system tools. Optional Cargo extensions remain ahead of
System directories; the host's ambient PATH is never inherited.

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
explicitly offline with shared read-only native data, HOME/admin state are scratch and
build output persists at /workspace/target. Missing dependencies require host provisioning; sandbox builds
never download."

## Native Test Scheduling

`native_repository_tests_execute_inside_the_workspace_sandbox` grants the actual
checkout as its writable scope. Other tests create temporary project fixtures
and `.skott-rust-*` resources under that checkout; their rustup registrations
deliberately target sandbox-only `/rust/toolchains/N` paths. Those links escape
the repository on the host, so the repository-wide trial correctly rejects
them while they exist. Do not bypass or ignore them in sandbox validation.

With `cargo test`, `#[serial]` coordinates marked tests within one test process.
Nextest starts each test in a separate process, so that attribute does not
provide nextest-wide exclusion. A serial test group also excludes only its own
members, not other tests.

The repository's tracked `.config/nextest.toml` selects the exact package, binary and
repository trial and uses `threads-required = "num-test-threads"` in the default
profile. This reserves the whole current nextest pool, even when `-j` changes
its size, and is inherited by the CI profile. Ordinary tests remain parallel.
Use the same override pattern for future tests that must run alone; change the
selector to their exact identity. Alternate configs or overriding profile rules
must retain this policy.

Normal nextest runs, `just test`, `just test-profile` and workspace CI
automatically discover this configuration in a clean checkout. No extra
`--config-file` argument is needed. This is the only tracked exception under
`.config`; other local settings remain ignored.

For a one-off isolated run:

```bash
cargo nextest run --offline --locked \
  -p skott --test rust_build_environment \
  -E 'test(=native_repository_tests_execute_inside_the_workspace_sandbox)'
```

`--test-threads 1` instead serializes every selected nextest test. Neither
approach coordinates another simultaneous runner invocation or unrelated host
writers. Interrupted tests can leave fixtures behind; exclusivity does not
remove stale directories. Inspect and clean only known abandoned fixtures
before rerunning, rather than weakening the scope checks.

## Offline dependencies

`kvist vendor` prepares package-manager-native offline resources. It fetches
missing material explicitly and reuses useful existing data; it does not impose
one directory-source format on every language. Resources are read-only during
verification, with private writable administrative/build state and OS-level
network denial. See [Offline dependencies](offline-dependencies.md) for exact
manager selection, paths and language limitations.

### Rust (Cargo)

1. Run `kvist vendor .` or `skott --vendor`: reuse the native cache, fetching
   only missing locked sources. Configured acquisition uses a fresh private
   administrative home and the bounded allowlisted fetch sandbox; failure does
   not authorize a host fallback.
2. Author with offline Cargo against read-only native roots. Skott workspace
   locks may update; protected maerg wrappers and task verification require
   `--offline --locked`. Verification uses `/workspace/cargo-registry` and
   `/workspace/cargo-git`, separate from authoring's `/rust/*` paths.

**To add or change dependencies:**

1. Edit `Cargo.toml` to add or change dependency specifications.
2. Run `cargo update` or `cargo add` on the host to update `Cargo.lock`.
3. Run `kvist vendor .` to prepare missing native cache material.
4. Commit `Cargo.toml`, `Cargo.lock`, and any changed shared vendor policy.
   Generated caches and small receipts remain ignored.

The sandbox build will then use the updated dependencies. No network access
is needed inside the sandbox.

**Key constraints:**

- Protected preparation requires a current `Cargo.lock` and available native
  source/index metadata. Ordinary workspace authoring is offline but not locked.
- Sandbox builds never download crates. Missing/corrupt native material fails
  with its actual cause. An explicit preparation action may repair missing data.
- Dependency build scripts/procedural macros run only as part of the isolated
  build, not during Rust `cargo fetch` acquisition.

### Agent dependency requests (`request_dependency`)

The tool can report dependency requests, but automatic model-requested fulfillment
currently fails closed: exact request-to-lock binding is not implemented.
No request triggers a hidden host package-manager fallback or default vendor
repacking. Explicit human provisioning remains separate.

### Other supported languages

| Language/manager | Offline resources and useful work |
| --- | --- |
| Go | Native selected `GOMODCACHE` mounted read-only at `/workspace/go-modules`; download missing modules without changing `go.mod`/`go.sum`. Verification uses `go test -mod=readonly ./...`, local toolchain, network/proxy/sumdb off and private build cache. No routine `go mod vendor` tree. |
| npm / yarn | Reuse the project's native `.kvist/vendored-js` cache and reconcile the locked project `node_modules`; do not copy a global cache wholesale. |
| pnpm | Populate `.kvist/vendored-js` directly with a frozen lockfile and native `--store-dir`; preserve it incrementally. Project `node_modules` installation is necessary. Arbitrary host global stores are not newly granted or claimed zero-copy. |
| pip / uv | Reuse the project wheelhouse and installed virtualenv; `uv.lock` is exported with `uv export --locked`, not fabricated. Wheels/venv installation remain useful materialization, not a Rust-style expanded native source cache. |
| Conan | Reuse the project-native Conan home and generated build files; required binary acquisition/build remains useful work. |
| vcpkg | Reuse the project-installed vcpkg root and native binary-cache capabilities; provisioning the required tool/installed packages remains useful work. |
| JVM / Ruby | System/zero-dependency support only; Maven/Gradle/Gem home-cache backends are not implemented. |

Update each manager's manifests/locks explicitly on the host, run `kvist vendor`,
then commit the inputs and optional policy, not generated caches. Native manager
formats have different installation and integrity rules; none requires a Kvist
per-source-file checksum manifest. A universal global-cache mount is not safe:
credentials, writable administrative state and installed hardlink aliases need
language-specific authority and lifecycle design.

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
KVIST_GALLA=/opt/target/debug/galla-runner \
  cargo test --locked -p skott --test rust_build_environment -- \
  --test-threads=1 --nocapture
```

Replace `/opt/target` if using a different outside-worktree target directory.
The host must provide a trusted system or host-home Rustup launcher and installed
standard/custom-linked toolchain outside the work scope. Complete-chain tests
also need rustfmt/Clippy, system cc/ar/as and working Bubblewrap; missing optional
companions do not disable independent compiler/source operations.
The CI quality job supplies these prerequisites and runs this chain explicitly.
No provider, network acquisition or pre-existing directory-export tree is needed for the
self-contained fixture tests.

Run every maerg native toolchain trial with
`KVIST_GALLA=/opt/target/debug/galla-runner cargo test --locked -p kvist --test sandbox_toolchain -- --include-ignored --test-threads=1 --nocapture`.
Its generic verification trials explicitly grant concrete installed executables,
and the build trial compiles a real isolated locked fixture. The separate
`offline_cargo_verification_e2e` covers native offline sources and pinned
toolchains; do not conflate those authority paths.

For alternate installed channels, set `KVIST_RUST_TEST_CHANNEL=nightly` and run
`selected_toolchain_runs_the_complete_native_chain -- --ignored --nocapture`.
For a minimal installation without rustfmt, set
`KVIST_RUST_TEST_MINIMAL_CHANNEL=1.95.0` and run
`installed_minimal_pin_builds_without_optional_components`; this also verifies
that subsequently requesting the absent formatter fails actionably.
Run `native_repository_tests_execute_inside_the_workspace_sandbox` only after
preparing this repository's native Cargo cache; it compiles and executes the
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
runs these otherwise ignored tests. The system-language trial selects host
executables from the same fixed `/usr/local/bin:/usr/bin:/bin` PATH it submits
to the sandbox; unrelated host PATH entries (including runner games and ghcup
directories) are not imported. Provision system tools in those bin directories,
with their resolved executables and Go runtime under the filtered System roots.
`skott/tests/rustup_inventory.rs` additionally
drives actual automatic Skott preparation, compares every installed version and
target, verifies compiler/doc selection under all three Cargo selection forms,
and checks denied mutations and unchanged host installations. CI explicitly
runs it in ordinary workspace tests using the same separately provisioned
multi-version/cross-target prerequisites. This inventory test and maerg's
`rust_authoring_has_host_inventory_private_defaults_and_offline_builds` are
**non-ignored**: ordinary `cargo test` runs them and missing runner/backend/
Rust prerequisites fail rather than skip. Both compare the host default and
inventory, exercise offline selection/builds, and snapshot host content and
metadata excluding access times. Build `galla-runner` outside the worktree first,
then run:

```bash
KVIST_GALLA=/opt/target/debug/galla-runner \
  cargo test --offline --locked -p skott --test rustup_inventory
KVIST_GALLA=/opt/target/debug/galla-runner \
  cargo test --offline --locked -p kvist --test sandbox_toolchain
```

Quality and MSRV CI provision these prerequisites on the host before ordinary
workspace tests. Tests never install anything or acquire dependencies.
Maerg's ordinary
`rust_authoring_builds_workspace_members_with_readonly_provider_context`
also builds an inherited workspace with a sibling provider, formats the
authored crate when the host default includes rustfmt, recompiles edited source,
executes a newly authored integration test, checks nested provider intent/state
exclusion and verifies that provider source/root lockfile are unchanged.
See the Galla toolchain testing review (retained as a work document) for
findings, limitations, recorded results and reproduction instructions.
