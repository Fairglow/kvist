# ADR 0013: Per-language offline verification topology

## Status

Accepted (decision in place; per-language implementation and evidence tracked in
`engine/TODOS.yaml`). The decision is that every supported non-Rust language
verifies through **one shared offline topology** — the existing generic
sandbox path extended with the language's vendored read-only mounts and a
writable scratch — rather than a bespoke closed topology per language. This
document records the decision and its rationale; it is not a compliance
certification.

Implemented:

- **`kvist vendor` dispatches per detected strategy.** Rust keeps its
  `cargo vendor` pass and versioned manifest; Go runs `go mod vendor`;
  JavaScript runs `npm ci` (or `yarn install --frozen-lockfile`) into the
  vendored package cache; Python downloads the locked wheels and provisions a
  virtualenv (offline install into it); C/C++ fills a project-local Conan home
  and generates the build files under `.kvist/conan-build`.
- **The shared offline language topology**
  (`engine::language_verification::run_offline_language_verification`):
  vendoring is enforced before any sandbox work, the language's vendored
  material is mounted read-only with the lock-file digest identity (ADR-0011),
  one disjoint writable scratch absorbs caches, build output, and `HOME`, the
  network is denied, and the language's canonical offline test command runs
  against the host system toolchain the runner already binds read-only. The
  writable scratch reuses the closed Cargo topology's fixed destination
  (`/workspace/scratch`) and endpoint-identity binding.
- **Per-language profiles.** Go: `go test -mod=vendor ./...` with
  `GOPROXY=off`, `GOTOOLCHAIN=local`, `GOFLAGS=-mod=vendor`, and
  `GOCACHE`/`GOTMPDIR`/`GOPATH`/`GOMODCACHE` under the scratch. JavaScript:
  `node --test`. Python: `python3 -m unittest -v` with `VIRTUAL_ENV` at the
  mounted provisioned venv (bytecode and user-site writes disabled). C/C++:
  the approved project test command (the build system is project-defined) with
  `CONAN_HOME` at the Conan home's canonical host path.
- **Go end-to-end evidence.** `engine/tests/language_offline_e2e.rs` vendors a
  real small Go module on the host (`kvist vendor` dispatch) and runs
  `go test -mod=vendor ./...` network-denied inside the Bubblewrap sandbox;
  the test self-skips without the live sandbox or the Go toolchain.

Validated live (security-sensitive, requires the bwrap runner environment):

- **Go verifies offline under the real bwrap runner** (see the end-to-end
  evidence above). The JavaScript, Python, and C/C++ profiles and provisioning
  passes are implemented at the enforcement and dispatch layers; their
  end-to-end evidence is tracked per language in `engine/TODOS.yaml` and is
  required before each is claimed as vendored-supported.

## Context

ADR-0011 makes every supported language own its vendored material: a lock file
as the authoritative catalogue, a host provisioning pass that may touch the
network, and fail-closed enforcement on every verification. The Rust language
has a closed verification topology for it (toolchain, vendored registry, cargo
config, runtime bin, scratch). The other supported languages have no comparable
closed topology, and a bespoke one per language would multiply the surface that
the runner validates and the engine plans.

The generic sandbox path already provides what these languages need for the
toolchain: the runner mounts the host system layout read-only
(`/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`), denies the network, and bounds the
request by an approved policy identity. What it lacks is the vendored material
(and, for Python, a runtime), plus writable space for caches and build output.

## Decision

Verify every non-Rust language through one shared offline topology that is the
generic sandbox path plus:

- **The language's vendored read-only mounts**, each identified by the
  lock-file digest, exactly as ADR-0011 defines for Rust:
  - **Go** — none: the vendored material is the committed `vendor/` directory,
    which travels inside the component mount; `go test -mod=vendor` builds
    entirely from it and needs no resolver configuration.
  - **JavaScript** — the vendored package cache (`.kvist/vendored-js`) plus the
    generated offline `.npmrc`.
  - **Python** — the vendored wheels (`.kvist/vendored-python`), the generated
    offline `pip.conf`, and the provisioned virtualenv (`.kvist/venv`) mounted
    at a fixed destination and made the interpreter's site via `PYTHONPATH`
    (CPython discovers a venv from the `pyvenv.cfg` beside its executable,
    not from `VIRTUAL_ENV`, so the host interpreter is pointed at the venv
    explicitly).
  - **C/C++ (Conan)** — the project-local Conan home (`.kvist/vendored-conan`)
    mounted **at its own canonical host path** (source equals destination), so
    the absolute cache paths embedded in the generated toolchain file resolve
    unchanged in the sandbox.
- **One writable scratch** at the fixed destination the closed Cargo topology
  uses, granted read-write with an endpoint identity derived from the host
  source path. Toolchain caches, build output, and `HOME` live under it.
- **The language's canonical offline test command** (or, for C/C++ only, the
  approved project test command, since the build system is project-defined):
  - Go — `go test -mod=vendor ./...`, with `GOPROXY=off` and
    `GOTOOLCHAIN=local` so any accidental fetch or toolchain download fails
    immediately.
  - JavaScript — `node --test`, running the project's `node:test` suites from
    the provisioned `node_modules`.
  - Python — `python3 -m unittest -v` with the mounted venv's site-packages on
    `PYTHONPATH`.
  - C/C++ — the approved `[test_policy]` command (for example `make test`)
    driving the project build system against the mounted Conan home.

The topology is fail-closed: vendoring is enforced before any sandbox work
(missing, incomplete, or stale material is a `VendoringUnavailable`/
`VendoringIncomplete` error, never a fallback to a host build), the sandbox
`PATH` is the fixed `/usr/bin:/bin` (no host search path leaks in), and the
language's host binary is located on the host and canonicalized so the
toolchain grant binds a regular, non-symlink executable. An approved test
command is rejected for the canonical-command languages (Go, JavaScript,
Python): their verification is the same for every project, which is what makes
the evidence uniform.

## Where actions are performed

| Action                                                                              | Performs it                                         | Network                | Boundary     |
| ----------------------------------------------------------------------------------- | --------------------------------------------------- | ---------------------- | ------------ |
| `go mod vendor`, `npm ci`, `pip download`, `uv venv`/`pip install`, `conan install` | Host, authorized provisioning step (`kvist vendor`) | Approved sources only  | Provisioning |
| Vendoring enforcement (lock-file digest, presence, mounts)                          | `kvist` engine (host)                               | None                   | Authority    |
| Canonical offline test command (or approved C/C++ test command)                     | Effect sandbox                                      | None (denied)          | Isolation    |
| System toolchain (`go`, `node`, `python3`, `gcc`/`make`)                            | Effect sandbox                                      | None (read-only mount) | Isolation    |

## Rationale

- **One topology, many languages.** The variation between these languages is
  the vendored material and the test command, both data; the sandbox shape is
  constant. A single topology means one mount plan, one scratch contract, and
  one set of runner validations, with per-language differences isolated in the
  profile.
- **Preserves the ADR-0011 model.** Provisioning on the host, lock-file digest
  identity, fail-closed enforcement, and durable `.kvist/` state are unchanged;
  the sandbox remains network-denied and reads only what the lock file catalogs.
- **Uniform evidence.** Canonical commands mean the end-to-end evidence per
  language is one self-skipping test with one expected outcome, exactly the
  shape `REQ-LANGUAGE-SUPPORT` requires.
- **No new runner surface.** The scratch grant reuses the destination and
  endpoint-identity binding the closed Cargo topology already validates; the
  vendored mounts reuse the lock-file-digest directory-mount identity.

## Alternatives considered

- **A closed topology per language** (the Rust shape): maximally strict, but
  the non-Rust languages have no equivalent of the cargo registry/config
  machinery to close around, so the added strictness is mostly ceremony; the
  shared topology already denies the network and binds every mount.
  Rejected for these languages; Rust keeps its closed topology.
- **Leave non-Rust verification on the plain generic path (no vendored
  mounts, system packages only):** works for zero-dependency projects but
  cannot support projects with third-party dependencies offline, which is the
  common case. Rejected.
- **Run the approved project test command for every language:** flexible, but
  verification would then differ per project and the uniform end-to-end
  evidence `REQ-LANGUAGE-SUPPORT` requires would not exist. Only C/C++ has a
  genuinely project-defined build system, so it is the exception.

## Consequences

- A supported language is claimed only with its self-skipping end-to-end test
  passing where the toolchain and live sandbox exist (Go now; JavaScript,
  Python, and C/C++ tracked per language).
- `kvist vendor` is no longer Rust-only: it provisions the detected language's
  vendored material, and every subsequent verification re-enforces it.
- The writable scratch at the Cargo destination is now shared by the closed
  Cargo topology and the language topology; the two never run concurrently for
  one request, and the destination is disjoint from the component mount.
- Provisioning limitations are documented per language (for example
  `uv.lock`-only Python projects fail closed until `uv.lock` provisioning is
  implemented; C/C++ requires an approved test command).
