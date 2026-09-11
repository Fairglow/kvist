<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observed component status

The Rust package builds a library and an executable named `kvist-sandbox-runner`.
The runner strictly parses and validates the version-one sandbox protocol and
enforces isolated execution via Bubblewrap.

## Observed command behavior

- `--kvist-sandbox-probe-v1`: Evaluates host kernel namespace availability (mnt, net,
  pid, ipc, uts, user) and verified `bwrap` executable path and SHA256 digest,
  emitting a confirmed `SandboxProbe` JSON response on standard output with status 0.
- `--kvist-sandbox-request-v1`: Reads bounded JSON requests from standard input,
  validates grants, bounds, and origin policies, and executes authoring, verification,
  or acquisition phases within isolated Bubblewrap namespaces under `prlimit` caps.
  Invalid requests fail closed with status 2; missing kernel/backend prerequisites exit
  with status 3.

## Implemented subsystems

- Version-one protocol request parsing, bounds checking, and strict schema validation (`src/protocol.rs`, `src/validation.rs`).
- Origin parsing, canonicalization, loopback classification, and allowlist matching (`src/origin.rs`).
- Descriptor-relative immutable Cargo-home generation staging and `RENAME_NOREPLACE` atomic publication (`src/cache.rs`).
- Host kernel namespace and Bubblewrap backend capability probe (`src/probe.rs`).
- Bubblewrap namespace isolation, `prlimit` resource capping, proxy mediation, and execution supervision (`src/enforcement.rs`).
