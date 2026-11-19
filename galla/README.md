# galla — the independent Linux enforcement boundary

`galla` is the independently installed Linux enforcement component for approved
Kvist task grants. It strictly parses and validates the version-one probe and
request contracts, rejects every superseded ("legacy") shape, implements the
native Cargo data-cache validation and separate portable-export generation
primitives, and enforces
approved requests through Bubblewrap-backed Linux isolation: user, IPC, PID,
UTS, cgroup, and network namespace boundaries, `prlimit` resource caps,
source-aware network egress mediation, and process supervision.

`galla` owns independent request validation and operating-system enforcement.
It does not authorize grants, transition tasks, promote output, or mint Kvist
evidence. It never falls back to unconstrained host execution and never reports
an unconstrained request as enforced; whenever verified enforcement
prerequisites are unavailable, every otherwise valid request fails closed.

`galla` is a standalone binary (`galla`) that is independent of the
`kvist` CLI. It is consumed by the `kvist` CLI (`maerg`) and by `skott` as the
enforcement boundary, and it can be probed and exercised on its own. Nothing
imports its types as a Rust library; the maerg mirrors the protocol
independently rather than depending on the runner, and the two communicate only
through the versioned wire contract.

## Cargo source modes

Native cached data is the normal protected Cargo mode. A present `cache.native`
selects it explicitly, even when its registry/Git endpoints are absent:

- Registry index, archives and expanded sources bind at
  `/workspace/cargo-registry`; Git data binds at `/workspace/cargo-git`.
  Verification reads these approved persistent roots read-only. Acquisition
  writes directly to the approved roots; it does not publish another copy.
- `CARGO_HOME` is exactly `scratch.destination + "/cargo-home"`, with private
  writable namespace-local administration and sandbox-only links to declared
  data mounts. An absent endpoint has an empty read-only data directory, not a
  writable fallback cache.
- Test verification is exactly `<cargo> test --offline --locked`, with denied
  network. Acquisition is exactly `<cargo> fetch --locked`, with approved
  package sources and an exact lockfile-before precondition. Bare `fetch` and
  `test --locked` are retired request spellings.
- No operator Cargo home, credentials, ambient configuration, native source
  inventory, cache copying or inferred promotion is used. Native writer/GC
  coordination is the shared provider's responsibility.

Portable vendor/export mode remains deliberately supported when `cache.native`
is absent/null and the caller supplies its explicit whole-home endpoints.
Export verification uses an approved read-only Cargo home plus the fixed
Registry `/workspace/vendored` and CargoConfig `/workspace/.cargo` grants;
only this mode describes `CARGO_HOME` itself as read-only. Export acquisition
uses its separate writable attempt Cargo home and export promotion intent.
Both modes use the exact locked commands above. Export is not a silent native
fallback, a migration path, or authority to invent a publication destination.
See [the contract](CONTRACT.md#native-cargo-source-mode) for the closed fields,
environments and grant counts.

Native source-only admission additionally permits exactly
`<cargo> metadata --offline --locked --format-version 1 --all-features` in the
verification phase with the same native read-only grants/private admin and
denied network. It resolves metadata without executing build scripts; stdout
is actual bounded Cargo JSON for the caller to validate, not a readiness
receipt. Portable exports and acquisition do not admit this operation.

Named registries use optional native `registries` descriptors
`[{ "name": "alt", "index_origin": "https://registry.example/index/", "protocol": "sparse" }]`.
Protocol is `"git"` or `"sparse"` (omission defaults to sparse). Git preserves
the exact HTTPS index URL without requiring a trailing slash; sparse requires
a trailing slash and adds Cargo's `sparse+` prefix. The caller must disambiguate
protocol from exact locked source identities, not silently convert Git indexes.
The caller supplies approved additional registry sources; acquisition requires
an exact name/index match. Galla generates only bounded named registry index
entries in private read-only Cargo config, never loads operator config or
replaces sources. Verification stays offline/network-denied.

The real Git-index regression publishes an owned Cargo package to a local Git
registry fixture and lets Cargo populate native data. Preparation uses only an
owned file repository and loopback archive server; the protected metadata run
has read-only native data, generated private config and denied network:

```bash
cargo test --offline --locked -p galla --test native_cache native_git_registry_metadata -- --ignored --test-threads=1 --nocapture
```

It requires Rust, Git and Bubblewrap, not a preexisting operator cache. This
proves cached Git-index resolution, not production HTTPS acquisition.

## Building

`galla` is a Rust 2024 package with MSRV Rust **1.95**. It builds a library
crate (`galla`) and the `galla` executable:

```bash
# Build the runner
cargo build -p galla

# Release build
cargo build --release -p galla

# Run the test suite
cargo test -p galla
```

No external services are required to build or test `galla` itself. The
enforcement path requires a verified Bubblewrap executable and Linux namespace
support at run time; the probe reports availability honestly and fails closed
when those prerequisites are absent.

## Running `galla`

`galla` accepts one of the protocol modes, or a command line to run inside the
sandbox:

```bash
# Confirm production enforcement capabilities (verified Bubblewrap backend and
# namespace enforcement). Emits a confirmed probe response, or fails closed.
galla --kvist-sandbox-probe-v1

# Read one bounded JSON request from standard input, validate it, and enforce
# it via Bubblewrap-backed Linux isolation.
galla --kvist-sandbox-request-v1

# Run a command line inside the sandbox. The current working directory is
# mounted read-write at /workspace and the standard system tree read-only at
# /usr; nothing else is mounted. The command is resolved to an absolute path
# (relative names are searched on the host PATH). The explicit `exec` keyword
# is equivalent.
galla /usr/bin/bash
galla bash -c 'echo hello'
galla exec /usr/bin/ls -la

# Run a Rust command with the host Rust installation mounted. The `--rust`
# flag takes the host Rust root directory and mounts its runtime, toolchain,
# rustup home, offline registry, git dependencies, and user cargo binaries
# read-only at the same /rust/* destinations Skott uses. A user home
# directory /home/<username> is mounted as a writable tmpfs (the username is
# inherited from the host USER environment variable), and the Rust setup runs
# before the payload, bridging the read-only /rust/* mounts into $HOME/.rustup
# and $HOME/.cargo so rustup and cargo find their state where a normal system
# would have it. Relative Rust command names (cargo, rustc, ...) are resolved
# from the Rust bin directories.
galla --rust /rust cargo build
galla --rust /rust rustc --version
galla exec --rust /rust cargo test --offline

# Run a Go, Node, or Python command with the host package cache mounted. The
# `--go`, `--node`, and `--python` flags take the host cache directory and
# mount it read-only at the expected $HOME-relative location ($HOME/go,
# $HOME/.npm, $HOME/.cache/uv), so the tool finds its cache where a normal
# system would have it. The interpreters and compilers (go, node, npm,
# python3, pip3, uv) live in the system tree and need no flag; only the
# offline cache does. The same /home/<username> home directory is mounted.
galla --go /root/go go build
galla --node /root/.npm npm install --offline
galla --python /root/.cache/uv uv build

# Combine toolchains: the same home directory is shared, and each cache is
# mounted at its expected location.
galla --rust /rust --go /root/go bash
```

The exec mode constructs a closed authoring-phase request — denied network,
the working directory as the only writable grant, the system tree read-only,
and the standard resource limits — and enforces it through the same
Bubblewrap-backed isolation as the one-shot request mode. It never falls back
to host execution. When any toolchain flag is given, a user home directory
`/home/<username>` is mounted as a writable tmpfs (the username is inherited
from the host `USER` environment variable), the toolchain caches are mounted
read-only into their expected `$HOME`-relative locations, and the environment
is extended with the toolchain variables. The sandbox resembles a normal
system: the tools find their caches where they would on a host. The flags are
opt-in: without them, only the system tree and the working directory are
mounted, and `HOME` is the working directory.

The accepted target interface for the protocol modes is the redefined
`kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` protocol described by
[ADR 0004](../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md). Request
parsing, validation, and Bubblewrap-backed isolation are fully implemented and
active.

Nested Galla invocations are rejected with status 3 and a clear "already inside
a Galla sandbox" diagnostic, before waiting for request input. Run the intended
command directly in the existing sandbox instead. Detection uses a read-only
namespace-local marker, not inherited environment variables. This notice does
not authorize bypassing an enclosing tool's execution policy. The exec mode
fails closed with the same diagnostic when invoked from inside a sandbox.

## Documentation

- [Requirements](REQUIREMENTS.md) — functional and non-functional requirements
- [Contract](CONTRACT.md) — protocol and enforcement semantics
- [Design](DESIGN.md) — internal structure and enforcement path
- [ADR 0004 — Bubblewrap runner and sandbox v1](../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md)
