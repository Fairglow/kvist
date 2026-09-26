# ADR 0012: Pinned Rust toolchains provisioned on the host

## Status

Accepted (decision in place; implementation tracked in
`engine/TODOS.yaml`). The decision is that the Rust toolchain used by the
offline Cargo verification topology is a **pinned, host-provisioned,
versioned** artifact rather than an ambient default. This document records the
decision and its rationale; it is not a compliance certification.

## Context

The offline Cargo verification topology (ADR-0011) mounts an immutable Rust
toolchain read-only so a network-denied `cargo test --locked` can compile and
link. The original resolution, `resolve_cargo_toolchain`, ran
`rustup which cargo` and used whatever toolchain rustup happened to select.
That behavior leaves the toolchain **unpinned and unmanaged**:

- **No alternate toolchains.** A project cannot require a specific compiler
  version; the sandbox silently uses the host default, so a build that is
  correct on one machine can differ on another.
- **No upgrade/downgrade story.** Moving a project to a newer (or older)
  compiler is an untracked side effect of the host environment, invisible to
  the workflow and to compliance.
- **No recorded state.** The chosen toolchain is not durable, inspectable
  state, so the build is not reproducible from the version-controlled project
  alone.
- **Authoring gap.** The authoring phase exposes only a read-only `/usr`
  layout (a `rustup` stub, not a real toolchain), so an agent cannot compile
  Rust while authoring. The toolchain must exist, on the host, outside the
  sandbox, before any phase can mount it.

The user's direction: support alternate toolchains and upgrade/downgrade as a
**host-side step performed outside the sandbox, not during a build**.

## Decision

Treat the Rust toolchain as a pinned, host-provisioned artifact with durable,
versioned state, mirroring how the vendored registry is treated in ADR-0011.

- **Pin in the project.** The toolchain is selected by a version-controlled
  `rust-toolchain.toml` (or `rust-toolchain`) at the project root, using
  rustup's native channel syntax (e.g. `1.95.0`, `stable`,
  `nightly-2026-01-01`). When absent, the host rustup default is used and that
  fact is recorded. The pin is the single source of truth for which compiler a
  build may use.
- **Provision on the host, outside the sandbox.** A distinct host-authorized
  step (`kvist toolchain ensure [PROJECT_DIR]`, alongside `kvist vendor`)
  resolves the pinned channel, runs `rustup toolchain install <channel>` when
  the channel is absent (this is the upgrade/downgrade path), and validates the
  resolved toolchain layout. This is the only step that may contact the network
  to obtain a toolchain, and it runs outside the effect sandbox, exactly as
  `kvist vendor` does for dependencies. Builds and verification never install,
  upgrade, or modify a toolchain; they only consume an already-provisioned one.
- **Record a versioned manifest.** The step writes
  `.kvist/rust-toolchain.json` recording the channel, the resolved toolchain
  root, the exact cargo path, and the cargo executable's content digest
  (the identity the sandbox request already uses). The manifest is durable,
  inspectable state under the Kvist-owned `.kvist/` directory.
- **Enforce on use.** Verification (and, once the authoring gap is closed,
  authoring) resolves the toolchain from the pin, re-checks it against the
  manifest, and fails closed with an actionable message when the pin is
  unsatisfied, the toolchain is absent, or the recorded digest no longer
  matches the on-disk toolchain. Resolution is channel-explicit
  (`--toolchain <channel>`), so it is independent of the process working
  directory and of any ambient rustup override.

## Where actions are performed

| Action                                        | Performs it                    | Network                                              | Boundary    |
| --------------------------------------------- | ------------------------------ | ---------------------------------------------------- | ----------- |
| `rustup toolchain install <channel>`          | Host, authorized provisioning step | Rust distribution origins (host-authorized)       | Provisioning |
| Toolchain manifest record and enforcement     | `kvist` engine (host)          | None                                                 | Authority   |
| Mounting the pinned toolchain into a build    | Effect sandbox                 | None (read-only mount)                               | Isolation   |

## Rationale

- **Reproducible and durable.** The pinned channel plus the recorded manifest
  make the exact compiler a version-controlled, inspectable fact, so a build is
  reproducible from the project alone and toolchain changes are visible to the
  workflow and to compliance.
- **Upgrades are a deliberate host step.** Moving to a newer or older compiler
  is an explicit, recorded, host-authorized action — never an in-build side
  effect and never an ambient environment accident.
- **Preserves the authority model.** No ambient toolchain authority enters the
  sandbox; the toolchain reaches it only as a read-only, identity-bound mount,
  exactly as the vendored registry does.
- **Mirrors ADR-0011.** Pinning, host provisioning, a versioned manifest, and
  fail-closed enforcement on use are the same shape as dependency vendoring, so
  the toolchain is managed with the model the project already trusts.
- **Unblocks the authoring gap.** Once the toolchain is a pinned, host-provisioned
  artifact with a known identity, it can be mounted into the authoring phase
  (a runner-contract extension, tracked separately) so agents can compile while
  authoring.

## Alternatives considered

- **Resolve the ambient rustup default at build time:** convenient but
  unpinned, unrecorded, and not reproducible; toolchain drift is invisible and
  there is no upgrade/downgrade story. Rejected.
- **Install toolchains inside the sandbox during a build:** gives the sandbox
  network and mutation authority over the compiler, defeats the authority model,
  and makes the build non-deterministic. Rejected.
- **Require the user to manage rustup by hand and only document it:** leaves the
  toolchain unrecorded and unenforced, so the build is not reproducible from the
  project and compliance cannot confirm the compiler. Rejected in favor of the
  provisioned, recorded, enforced model.

## Consequences

- A `rust-toolchain.toml`/`rust-toolchain` pin is the authoritative compiler
  selection; a project that needs a specific version records it there.
- `kvist toolchain ensure` is a new host-authorized step that may reach the
  network (Rust distribution) and writes a durable manifest; it is the only
  supported upgrade/downgrade path.
- Verification fails closed when the pinned toolchain is absent or has drifted
  from the recorded manifest, with an actionable message naming the missing
  step.
- The authoring-phase toolchain mount remains a follow-up that extends the
  shared runner contract; this ADR does not claim it is implemented.
- Toolchain selection becomes part of durable, inspectable state, consistent
  with the vendored-registry model.
