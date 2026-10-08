# ADR 0011: Vendored offline builds for sandbox verification

## Status

Accepted. The decision is in place: `kvist vendor` provisions and enforces an
offline vendored dependency registry, and the `maerg` vendoring module records
and re-checks a versioned manifest so a sandbox build can resolve dependencies
fully offline. This document records the decision and its rationale; it is not a
compliance certification.

Implemented:

- `kvist vendor` populates a vendored registry, reconciles it on lockfile drift,
  writes the versioned manifest (`.kvist/vendoring-v1.json`), and fails closed
  when a locked registry or Git dependency is missing or the lockfile has
  drifted. The manifest enforcement primitive (`VendorManifest::assert_ready`)
  is implemented and tested.
- **Directory read-only mounts with a lock-file digest identity.** The sandbox
  grant identity model was extended so a read-only mount may carry an explicit
  identity instead of hashing its source bytes. A directory source is mounted
  with `ReadOnlyMount::directory(source, destination, identity)` and the
  identity is the lock-file digest; single files still derive theirs from their
  bytes (`ReadOnlyMount::file`). This is what makes it possible to mount a
  vendored registry without hashing hundreds of thousands of files. See
  "Lock-file digest as the directory-mount identity".
- **Language-aware vendoring enforcement.** `maerg::language_vendoring`
  provides a `LanguageStrategy` trait and a single `enforce_vendoring`
  entry point that Kvist owns: Rust enforces each locked registry and Git entry
  exactly (through the existing `VendorManifest` machinery), while Python
  (`pip`/`uv`), JavaScript/Node (`npm`), and C/`Conan` enforce lock-file match
  plus vendored-content presence. `detect_language_strategy` prefers Rust so a
  Rust+JS project verifies against its `Cargo.lock`. Implemented and
  tested (unit tests plus an integration test that enforces against a real
  vendored project). See "Multi-language vendoring".
- **Route Rust verification through the Cargo topology and add the vendored
  mounts.** The closed verification topology (Toolchain, DependencyCache, Scratch,
  Verification) carries three read-only extensions: a vendored-registry mount at
  `/workspace/vendored`, a cargo-config mount at `/workspace/.cargo`, and a
  runtime-bin mount at `/workspace/bin`. The two vendored mounts are each
  identified by the lock-file digest so the mount plan is a build-time claim over
  the locked catalogue. The maerg routes Rust verification through
  `sandbox::run_offline_cargo_verification`, which enforces vendoring readiness,
  resolves the immutable toolchain, provisions a read-only approved Cargo home,
  a read-only runtime bin (symlinks exposing `rustc`, `rustdoc`, and the system
  linker/archiver `cc`/`ar`/`as` so a locked build can compile and link), and a
  disjoint scratch, and builds the seven-grant request validated by the runner;
  `verify_task` selects this path when `detect_language_strategy` reports Rust and
  falls back to the approved test command otherwise. The `Purpose::Registry`,
  `Purpose::CargoConfig`, and `Purpose::Runtime` grant purposes pin the mounts to
  their fixed, non-overlapping, read-only destinations. Implemented, unit-tested,
  and validated live against the bwrap runner by the end-to-end test
  `offline_cargo_verification_e2e` (a network-denied `cargo test --locked` that
  compiles, links, and runs inside bubblewrap).
- **Sandbox cargo configuration is authoritative; the project-local config is
  left untouched.** Cargo resolves configuration from the project-local
  `.cargo/config.toml` first, and that file is carried into the sandbox through
  the component mount. A host-path source config written there would shadow the
  mounted `/workspace/.cargo` config and break the offline build. `kvist vendor`
  therefore writes only the sandbox cargo configuration (at
  `.kvist/sandbox-cargo/config.toml`, mounted at `/workspace/.cargo`) and never
  writes source-replacement keys into the project's own `.cargo/config.toml`.
  Host builds resolve from the network as usual; only the sandbox build is
  required to be offline.

Validated live (security-sensitive, requires the bwrap runner environment):

- **The extended Cargo topology runs under the real bwrap runner.** The routing,
  the vendored mounts, and the runtime-bin mount are exercised end to end by
  `maerg/tests/offline_cargo_verification_e2e.rs`, which vendors a small project
  and runs a network-denied `cargo test --locked` inside bubblewrap that compiles,
  links, and executes. The test self-skips when the live sandbox cannot run
  (no built runner, no bubblewrap backend, no cargo/rustup, no git worktree, or no
  network for the initial vendoring pass) so it never fails in an environment that
  lacks the bwrap runner. Extending the closed topology admits new read-only
  authority and is documented here rather than certified.

- **In-sandbox acquisition provisioning.** When the registry is stale,
  `kvist vendor` runs a network-allow **dependency-acquisition sandbox**
  (`cargo fetch` with allowlisted package sources) *before* the effect sandbox
  executes, then repacks the fetched material into the vendored registry with an
  **offline** `cargo vendor --offline` (the host never resolves crate sources).
  The request is the closed four-grant `DependencyAcquisition` topology
  (toolchain, writable Cargo home, writable scratch, writable lockfile
  workspace) with `PackageSources` network authority, built by
  `sandbox::build_dependency_acquisition_request` and executed by
  `sandbox::execute_dependency_acquisition`; the runner promotes the fetched
  Cargo home after the fetch child exits. When a sandbox cannot run (no runner,
  backend, or toolchain) the pass falls back to the classic host `cargo vendor`
  so plain checkouts and CI keep working. Implemented and unit-tested: the
  acquisition request is validated against the shared runner parser/validator
  (`sandbox::tests::dependency_acquisition_request_satisfies_the_shared_runner_validator`),
  the fallback path is tested
  (`vendor_command::tests::acquisition_fallback_when_no_toolchain_is_resolvable`),
  and a live end-to-end test
  (`maerg/tests/offline_cargo_verification_e2e.rs::acquisition_sandbox_provisions_vendored_registry_before_verification`)
  drives the full in-sandbox provision → offline verify path (it self-skips
  where the live sandbox cannot run).

## Lock-file digest as the directory-mount identity

Read-only directory sources wired into the sandbox (the vendored registry, the
sandbox cargo configuration) can span hundreds of thousands of files. Hashing
every file to derive a mount identity is neither necessary nor worthwhile:

- The lock file is the authoritative catalogue of the locked content. Its
  SHA-256 digest is both the manifest identity and the identity of every
  read-only vendored-directory mount (`language_vendoring::lockfile_digest`).
- Read-only grant identities are a build-time claim bound to the approved mount
  plan; they are not re-verified at execution. The digest is the catalogue, and
  presence is enforced against it (`assert_ready`, or the per-language presence
  check). The lock file is therefore exactly the content summary the sandbox
  needs, and no per-file tree hash is computed.

`Cargo.lock` is the durable, version-controlled catalogue; the manifest digest
ties the on-disk vendored material to that catalogue. The manifest itself lives
under `.kvist/` (gitignored, regenerated and re-enforced each run), so vendoring
remains Kvist-owned and seamless: it is enforced, not hand-maintained.

## Multi-language vendoring

The vendoring model is language-agnostic at the enforcement layer and
Rust-first in detection. Each language provisions its exact locked material on
the host outside the sandbox and re-checks it against its own lock file:

- **Rust** — the network step is `cargo fetch` in a network-allow
  **acquisition sandbox** (allowlisted package sources), run before the effect
  sandbox executes; the fetched material is repacked into `.kvist/vendored` by
  an offline `cargo vendor --offline` (falling back to a host `cargo vendor`
  when no sandbox can run). Enforced exactly through `Cargo.lock`; offline
  config at `/workspace/.cargo`.
- **Python** — `pip`/`uv`: `requirements.lock.txt` or `uv.lock`; vendored
  wheels/sdists in `.kvist/vendored-python`; offline `pip.conf` at
  `/workspace/.pip`.
- **JavaScript/Node** — `npm`: `package-lock.json`; vendored tarballs in
  `.kvist/vendored-js`; offline `.npmrc` at `/workspace/.npm`.
- **C/Conan** — `conanfile.lock`; vendored packages in `.kvist/vendored-conan`;
  offline provisioning documented at `/workspace/.conan`.

Rust enforces every locked registry and Git entry exactly; the other languages
enforce catalogue match plus vendored-content presence, which is the honest and
cheap guarantee their lock files provide (exact per-package verification is
follow-up work). All four share one enforcement path (`enforce_vendoring`), one
identity scheme (the lock-file digest), and one offline-provisioning boundary
(the host, outside the effect sandbox).

## Context

Every sandbox phase is network-denied and the Rust toolchain reaches the sandbox
only as a read-only mount ([ADR 0005](0005-mediated-dependency-and-model-networking.md),
[ADR 0004](0004-bubblewrap-runner-and-sandbox-v1.md)). A `cargo` invocation
therefore cannot fetch dependencies: without material already on disk it cannot
resolve or compile a project. A task agent whose sandbox cannot build or test is
unable to perform the executor's primary purpose, since real coding work needs
the development toolchain, which under normal (non-vendored) operation requires
network access to acquire dependencies.

The host that runs `kvist` has network and the Rust toolchain, but that authority
must not live inside the effect sandbox. Unrestricted network to `cargo` inside
the sandbox, or running an unrestricted `cargo` as a model-facing sandbox tool,
is exactly what [ADR 0005](0005-mediated-dependency-and-model-networking.md)
rejects: process identity is not sufficient mediation when an agent can invoke
cargo or its subprocesses with attacker-controlled inputs.

## Decision

Provision the dependency material once, on the host, as a vendored registry, and
make the sandbox build from it offline.

- **Provision in a network-allow acquisition sandbox, before the effect sandbox
  executes.** When the vendored registry is stale (a locked dependency is absent
  and the registry does not satisfy the lock), `kvist vendor` runs a
  **dependency-acquisition sandbox** — a network-allow phase with allowlisted
  package sources (crates.io plus any project-approved sources) — that executes
  `cargo fetch` **before the effect sandbox executes**. This is the only step
  that may contact the network, and it runs inside a sandbox, not as an
  unrestricted host `cargo`. The fetched material is then repacked into the
  vendored registry by an **offline** `cargo vendor --offline` (network denied,
  so the host never resolves crate sources); the runner promotes the fetched
  Cargo home into a project cache after the fetch child exits. When a sandbox
  cannot run in the environment (no runner, backend, or toolchain), the pass
  falls back to the classic host `cargo vendor` so plain checkouts and CI keep
  working. Git dependencies must be immutable and pre-approved, restricted to
  the sources approved under
  [ADR 0005](0005-mediated-dependency-and-model-networking.md). When the
  registry already holds material it is reused and the acquisition pass is
  never run.
- **Record a versioned manifest.** `kvist vendor` writes `.kvist/vendoring-v1.json`
  under the project, recording the lockfile digest, the vendored directory, the
  sandbox `.cargo/config.toml` directory, and the fixed sandbox mount
  destinations. The manifest is version-controlled durable state, and it is the
  single authoritative record of what offline material is trusted, tied to the
  version-controlled `Cargo.lock` catalogue (see below).
- **Enforce vendoring.** Kvist owns vendoring. `kvist vendor` enforces readiness
  before it records the manifest, failing closed when a registry or Git
  dependency is absent. The manifest enforcement primitive re-reads the manifest,
  rejects a stale lockfile (digest no longer matches), and rejects missing
  dependencies. Sandbox verification mounts the vendored registry read-only at
  `/workspace/vendored` together with a sandbox cargo configuration at
  `/workspace/.cargo` and a runtime-bin directory at `/workspace/bin`. The cargo
  configuration maps the crates-io source at the vendored registry. The three
  mounts are distinct directories because the sandbox runner forbids overlapping
  the component mount. That verification wiring is implemented and validated
  (see Status).
- **Build offline in the sandbox.** `cargo build`/`cargo test --locked`
  execute inside the effect sandbox with network denied, resolving everything
  from the vendored registry with the toolchain mounted read-only. No
  acquisition cache or network is required for the build itself.
- **Run network operations in a mediated acquisition sandbox.** `cargo fetch`
  runs in a network-allow **dependency-acquisition sandbox** with allowlisted
  package sources, before the effect sandbox executes; `cargo vendor
  --offline` (the repack) and `cargo generate-lockfile` run as host-authorized
  steps under the [ADR 0005](0005-mediated-dependency-and-model-networking.md)
  acquisition model, never as unrestricted sandbox tools. The effect sandbox
  itself is always network-denied.

## Where actions are performed

| Action                                                         | Performs it                        | Network                                                                              | Boundary    |
| -------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------ | ----------- |
| `cargo fetch` (acquisition)                                    | Dependency-acquisition sandbox     | Approved sources only ([ADR 0005](0005-mediated-dependency-and-model-networking.md)) | Acquisition |
| `cargo vendor --offline` (repack), `cargo generate-lockfile`   | `kvist` maerg (host)              | None (offline)                                                                       | Acquisition |
| Vendoring manifest record and enforcement                      | `kvist` maerg (host)              | None                                                                                 | Authority   |
| `cargo test --locked`, `cargo build`                           | Effect sandbox                     | None (`deny` + offline + vendored registry)                                          | Isolation   |
| `cargo`, `rustc`, toolchain material                           | Effect sandbox                     | None (read-only mount)                                                               | Isolation   |

## Rationale

- **Preserves the authority model.** No ambient network or authority is given to
  the sandbox; network toolchain operations are a bounded, approved
  dependency-acquisition sandbox phase with allowlisted package sources.
- **Deterministic and reproducible.** The vendored registry holds the exact
  locked versions, so offline builds resolve identically every time.
- **Seamless for the user.** Once vendored, the agent builds and tests Rust with
  no network and no manual steps. Adding a dependency requires one pass
  (`cargo add`, then `kvist vendor`) that refreshes the registry and manifest;
  the network step (the `cargo fetch` in the acquisition sandbox) runs
  automatically, before the effect sandbox executes.
- **Durable, inspectable state.** The lock file (`Cargo.lock` and, for other
  languages, their respective lock files) is the durable, version-controlled
  catalogue; the manifest records its digest and the vendored layout, and Kvist
  re-enforces vendoring on every run, satisfying the recursive-component
  durability principle without hand-maintaining a large vendored tree.

## Alternatives considered

- **Allow sandbox cargo unrestricted network:** convenient, but defeats the
  authority and confidentiality model and is rejected by [ADR 0005](0005-mediated-dependency-and-model-networking.md).
- **Run an unrestricted `cargo` as a sandbox tool with ambient network:** process
  identity is not sufficient mediation and would let arbitrary agent-controlled
  subprocesses reach the network. Rejected in favor of a host acquisition phase.
- **Mediated acquisition cache only (no vendoring):** works but requires the
  acquisition phase each time dependencies change and is less reproducible.
  Vendoring makes the offline source a durable, version-controlled artifact and
  lets verification build without any acquisition step.
- **Mount the vendored registry at the component mount:** the sandbox runner
  forbids overlapping the component mount, so a separate vendored mount plus a
  dedicated sandbox cargo-config directory are used instead.

## Consequences

- Builds and tests run offline and deterministically, so the agent can perform
  real coding work inside the sandbox.
- A vendoring pass is required after dependency changes; the manifest enforces it
  and fails closed with a clear, non-secret message when the lockfile drifted or
  a dependency is missing.
- The vendored registry and manifest live under `.kvist/` (gitignored, so they
  are regenerated and re-enforced rather than hand-maintained); the durable,
  version-controlled catalogue is the lock file, and Git dependencies remain
  immutable and pre-approved.
- Project-level acceptance and advisory-review gates for project intent remain
  planned; this ADR does not represent them as implemented.
