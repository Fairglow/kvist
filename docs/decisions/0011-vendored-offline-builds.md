# ADR 0011: Vendored offline builds for sandbox verification

## Status

Accepted. The decision is in place: `kvist vendor` provisions and enforces an
offline vendored dependency registry, and the `engine` vendoring module records
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
- **Language-aware vendoring enforcement.** `engine::language_vendoring`
  provides a `LanguageStrategy` trait and a single `enforce_vendoring`
  entry point that Kvist owns: Rust enforces each locked registry and Git entry
  exactly (through the existing `VendorManifest` machinery), while Python
  (`pip`/`uv`), JavaScript/Node (`npm`), and C/`Conan` enforce lock-file match
  plus vendored-content presence. `detect_language_strategy` prefers Rust so a
  mixed Rust+JS project verifies against its `Cargo.lock`. Implemented and
  tested (unit tests plus an integration test that enforces against a real
  vendored project). See "Multi-language vendoring".

Planned (security-sensitive, requires the bwrap runner environment to validate):

- **Route Rust verification through the Cargo topology and add the vendored
  mounts.** The sandbox enforces an offline `cargo` build through a closed
  four-grant topology (Toolchain, DependencyCache, Scratch, Verification) and
  `build_verification_plan` constructs that topology, but it is not yet wired
  into production verification, and that closed topology does not yet carry the
  read-only vendored-registry and cargo-config mounts. Extending the closed
  topology to admit those mounts changes an enforcement invariant and is the
  planned integration; it is documented here, not shipped unvalidated.

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

- **Rust** — `cargo vendor` into `.kvist/vendored`, enforced exactly through
  `Cargo.lock`; offline config at `/workspace/.cargo`.
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

- **Provision on the host.** `kvist vendor` populates a vendored registry from
  the exact locked versions with `cargo vendor` and the project `Cargo.lock`,
  restricted to the sources approved under [ADR 0005](0005-mediated-dependency-and-model-networking.md).
  Git dependencies must be immutable and pre-approved. The vendored registry is
  the only step that may contact the network, and it runs outside the effect
  sandbox. When the registry already holds material it is reused and cargo is
  never invoked again.
- **Record a versioned manifest.** `kvist vendor` writes `.kvist/vendoring-v1.json`
  under the project, recording the lockfile digest, the vendored directory, the
  host and sandbox `.cargo/config.toml` paths, and the fixed sandbox mount
  destinations. The manifest is version-controlled durable state, and it is the
  single authoritative record of what offline material is trusted, tied to the
  version-controlled `Cargo.lock` catalogue (see below).
- **Enforce vendoring.** Kvist owns vendoring. `kvist vendor` enforces readiness
  before it records the manifest, failing closed when a registry or Git
  dependency is absent. The manifest enforcement primitive re-reads the manifest,
  rejects a stale lockfile (digest no longer matches), and rejects missing
  dependencies. Sandbox verification is intended to mount the vendored registry
  read-only at `/workspace/vendored` together with a sandbox cargo configuration
  at `/workspace/.cargo`. The cargo configuration maps the crates-io source at the
  vendored registry. The two mounts are distinct directories because the sandbox
  runner forbids overlapping the component mount. That verification wiring is the
  planned integration (see Status).
- **Build offline in the sandbox.** `cargo build`/`cargo test --locked`
  execute inside the effect sandbox with network denied, resolving everything
  from the vendored registry with the toolchain mounted read-only. No
  acquisition cache or network is required for the build itself.
- **Run network operations outside the sandbox.** `cargo vendor`,
  `cargo generate-lockfile`, and `cargo fetch`/`cargo add` run as host-authorized
  steps under the [ADR 0005](0005-mediated-dependency-and-model-networking.md)
  acquisition model, never as unrestricted sandbox tools.

## Where actions are performed

| Action                                                         | Performs it                        | Network                                                                              | Boundary    |
| -------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------ | ----------- |
| `cargo vendor`, `cargo generate-lockfile`, `cargo fetch`/`add` | Host, authorized acquisition phase | Approved sources only ([ADR 0005](0005-mediated-dependency-and-model-networking.md)) | Acquisition |
| Vendoring manifest record and enforcement                      | `kvist` engine (host)              | None                                                                                 | Authority   |
| `cargo test --locked`, `cargo build`                           | Effect sandbox                     | None (`deny` + offline + vendored registry)                                          | Isolation   |
| `cargo`, `rustc`, toolchain material                           | Effect sandbox                     | None (read-only mount)                                                               | Isolation   |

## Rationale

- **Preserves the authority model.** No ambient network or authority is given to
  the sandbox; network toolchain operations are a bounded, approved host phase.
- **Deterministic and reproducible.** The vendored registry holds the exact
  locked versions, so offline builds resolve identically every time.
- **Seamless for the user.** Once vendored, the agent builds and tests Rust with
  no network and no manual steps. Adding a dependency requires one host-side pass
  (`cargo add`, then `kvist vendor`) that refreshes the registry and manifest.
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
