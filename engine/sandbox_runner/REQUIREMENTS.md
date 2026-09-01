<!-- kvist-requirements-version: 1 -->
# Sandbox Runner Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`sandbox-runner` is the independently installed Linux enforcement component
for approved Kvist task grants. This initial component revision establishes a
buildable, adjacent scaffold only. It MUST NOT claim to implement the accepted
sandbox protocol or Bubblewrap enforcement before the queued test and
implementation tasks are completed.

## Stakeholders and concerns

- Kvist engine integrators need a separately packaged executable boundary.
- Security reviewers need fail-closed behavior until native enforcement exists.
- Operators need an honest distinction between a buildable scaffold and a
  production runner.
- Future implementers need the accepted ADR 0004 boundary retained without
  prematurely inventing protocol behavior.

## Functional requirements

The scaffold and later production boundary have separate, explicit
requirements so buildability cannot be confused with enforcement readiness.

### SR-REQ-SCAFFOLD

The component MUST build as a Rust 2024 package, expose one
`kvist-sandbox-runner` executable, keep its source and tests adjacent to local
intent, and report that runner functionality is unavailable. It MUST NOT
accept a probe or task request as successfully enforced.

### SR-REQ-PRODUCTION-RUNNER

The later implementation MUST follow
[`../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md`](../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md):
strict bounded version-one probe and request parsing, independent grant
validation, Bubblewrap-backed Linux isolation, explicit resource enforcement,
and fail-closed backend verification. This requirement is approved target
intent, not a claim about the scaffold.

## Quality requirements and constraints

- Rust stable edition 2024 is required and unsafe Rust is forbidden.
- Executable support is Linux-only.
- The component MUST NOT import Kvist engine implementation types.
- Repository input and future protocol input are untrusted.
- No shell, network service, daemon, credential, ambient home, or worktree
  runner installation is introduced by the scaffold.

## Acceptance and traceability

The local queue orders protocol tests, implementation, security audit, and
independent compliance review. Scaffold tests verify only package shape and
fail-closed unavailability. Production protocol and Bubblewrap evidence belongs
to those later tasks and MUST NOT be inferred from a successful scaffold build.
