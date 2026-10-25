# galla — the independent Linux enforcement boundary

`galla` is the independently installed Linux enforcement component for approved
Kvist task grants. It strictly parses and validates the version-one probe and
request contracts, rejects every superseded ("legacy") shape, implements the
mediated Cargo validation and immutable-generation primitives, and enforces
approved requests through Bubblewrap-backed Linux isolation: user, IPC, PID,
UTS, cgroup, and network namespace boundaries, `prlimit` resource caps,
source-aware network egress mediation, and process supervision.

`galla` owns independent request validation and operating-system enforcement.
It does not authorize grants, transition tasks, promote output, or mint Kvist
evidence. It never falls back to unconstrained host execution and never reports
an unconstrained request as enforced; whenever verified enforcement
prerequisites are unavailable, every otherwise valid request fails closed.

`galla` is a standalone binary (`galla-runner`) that is independent of the
`kvist` CLI. It is consumed by the `kvist` CLI (`maerg`) and by `skott` as the
enforcement boundary, and it can be probed and exercised on its own. Nothing
imports its types as a Rust library; the maerg mirrors the protocol
independently rather than depending on the runner, and the two communicate only
through the versioned wire contract.

## Building

`galla` is a Rust 2024 package with MSRV Rust **1.95**. It builds a library
crate (`galla`) and the `galla-runner` executable:

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

## Running `galla-runner`

`galla-runner` accepts exactly one protocol mode argument:

```bash
# Confirm production enforcement capabilities (verified Bubblewrap backend and
# namespace enforcement). Emits a confirmed probe response, or fails closed.
galla-runner --kvist-sandbox-probe-v1

# Read one bounded JSON request from standard input, validate it, and enforce
# it via Bubblewrap-backed Linux isolation.
galla-runner --kvist-sandbox-request-v1
```

The accepted target interface is the redefined `kvist-sandbox-probe-v1` and
`kvist-sandbox-request-v1` protocol described by
[ADR 0004](../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md). Request
parsing, validation, and Bubblewrap-backed isolation are fully implemented and
active.

Nested Galla invocations are rejected with status 3 and a clear "already inside
a Galla sandbox" diagnostic, before waiting for request input. Run the intended
command directly in the existing sandbox instead. Detection uses a read-only
namespace-local marker, not inherited environment variables. This notice does
not authorize bypassing an enclosing tool's execution policy.

## Documentation

- [Requirements](REQUIREMENTS.md) — functional and non-functional requirements
- [Contract](CONTRACT.md) — protocol and enforcement semantics
- [Design](DESIGN.md) — internal structure and enforcement path
- [ADR 0004 — Bubblewrap runner and sandbox v1](../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md)
