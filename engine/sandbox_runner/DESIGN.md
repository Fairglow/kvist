<!-- kvist-design-version: 1 -->
# Sandbox Runner Design

## Design overview

The initial crate is deliberately minimal: a small library exposes scaffold
status and the executable reports that enforcement is unavailable. Protocol
parsing and Bubblewrap production behavior remain excluded until their queued
tests exist.

## Internal structure

| Path | Responsibility |
| --- | --- |
| `src/lib.rs` | Honest scaffold status shared by tests and the executable |
| `src/main.rs` | Fail-closed command boundary |
| `tests/scaffold.rs` | Structural and unavailable-behavior evidence |

The later implementation will add private protocol, validation, Bubblewrap,
resource, process, and error modules without importing engine types.

## Interactions and state

The scaffold accepts no valid runner request and persists no state. The
production design will receive canonical JSON from the engine, validate it
independently, construct one explicit Bubblewrap execution, supervise it, and
return bounded results.

## Algorithms and decisions

The only scaffold decision is fail-closed unavailability. This prevents a
buildable placeholder from being mistaken for the accepted protocol. ADR 0004
is authoritative for the future version-one protocol and Linux enforcement
shape.

## Failure and recovery

Every scaffold invocation fails before task execution. It creates no attempt,
queue, approval, or evidence state and therefore has no recovery transition.
Future interruption and process-tree recovery behavior must be designed and
tested with the production implementation.

## Security and resource design

No shell is used, no child process is spawned, no request path is opened, and
no network or ambient credentials are accessed. The later runner must verify
its own installation and Bubblewrap identity, validate every typed grant, and
enforce the namespace and resource constraints accepted in ADR 0004.

## Verification strategy

Current tests assert the declared scaffold status and nonzero CLI result.
Queued protocol tests must precede implementation and cover malformed and
legacy requests, aliases and links, grant overlap, hidden workflow state,
Bubblewrap identity, namespace setup, resource exhaustion, process cleanup,
and refusal to fall back to host execution.
