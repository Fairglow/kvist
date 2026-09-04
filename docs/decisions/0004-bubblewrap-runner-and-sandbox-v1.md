# ADR 0004: Bubblewrap runner and redefined sandbox protocol version 1

## Status

Accepted

## Context

Kvist requires an independently installed enforcement boundary for effectful
tasks, but no production runner currently implements its probe and request
protocol. The existing protocol assumes one writable component mount and
cannot express separate authoring, verification, dependency, scratch, and
evidence authority. Kvist has not released that protocol and promises no
backward compatibility.

## Decision

Create `sandbox-runner` as a child component that produces one Linux
Bubblewrap-backed executable. Its installed executable must be a regular,
non-link file outside the selected project and worktree. The engine communicates
with it only through canonical bounded JSON and does not import runner
implementation types.

Redefine `kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` in place before
promotion. Temporary development probes may use an explicitly experimental
identifier, but the accepted durable protocol remains version 1. No legacy
version-one request or fallback runner is recognized after the redefinition.

The request contains typed grants for:

- canonical source and destination paths;
- read-only, read-write, scratch, toolchain, dependency-cache, and context
  mount purposes;
- one explicit working directory;
- exact program and argument vector;
- environment names and values supplied by the host;
- network capability profile;
- time, output, process, file, and scratch-storage limits; and
- the effective runner, Bubblewrap, toolchain, command, mount-plan, and policy
  identities covered by approval.

Authoring and verification are separate requests. Intent, queues,
implementation records, attempt evidence, review evidence, `.git`, ambient
home state, and credentials are absent or read-only and are never agent
writable. The runner validates grants and enforces them; it cannot authorize a
grant, transition a task, promote output, or mint evidence.

Bubblewrap is the first backend. It uses mount, network, PID, IPC, UTS, and
user namespaces, a new session, parent-death handling, minimal `/proc` and
`/dev`, empty temporary home state, and explicit resource enforcement. Missing
kernel support or an unverifiable backend fails closed.

## Alternatives considered

- **Keep the original version-one shape:** cannot represent the authority
  required by the Rust workspace or protect workflow files inside a component.
- **Publish version 2:** semantically clear, but creates a compatibility story
  where none exists. Redefining the unreleased version-one contract is simpler.
- **Docker or Podman first:** useful future backends, but add daemon/image,
  rootless-runtime, and supply-chain concerns to the first implementation.
- **Landlock alone:** useful defense in depth but not a complete process,
  mount, network, and context boundary.
- **Repository script runner:** too easy to replace and insufficiently typed
  for the enforcement contract.

## Consequences

All existing protocol tests and documentation are replaced rather than
migrated. Policy approval must bind the actual Bubblewrap binary and effective
profile, not only the wrapper. Linux is the sole executable platform until
another backend has independent native evidence.
