# ADR 0011: Vendored offline builds for sandbox verification

## Status

Accepted. The decision is in place: `kvist vendor` provisions and enforces an
offline vendored dependency registry, and the `engine` vendoring module records
and re-checks a versioned manifest so a sandbox build can resolve Rust
dependencies fully offline. This document records the decision and its rationale;
it is not a compliance certification.

Implemented: `kvist vendor` populates a vendored registry, reconciles it on
lockfile drift, writes the versioned manifest, and fails closed when a locked
registry or Git dependency is missing or the lockfile has drifted. The manifest
enforcement primitive (`VendorManifest::assert_ready`) is implemented and tested.

Planned: the sandbox-verification wiring that calls that primitive and mounts the
vendored registry and sandbox cargo configuration read-only. The current
read-only mount mechanism records only file identities, so mounting those two
directory sources requires extending the sandbox grant identity model first.

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
  single authoritative record of what offline material is trusted.
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
- **Durable, inspectable state.** The manifest under version control is the
  enforceable record, satisfying the recursive-component durability principle.

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
- The vendored registry and manifest are committed, version-controlled material
  that can be large, and Git dependencies remain immutable and pre-approved.
- Project-level acceptance and advisory-review gates for project intent remain
  planned; this ADR does not represent them as implemented.
