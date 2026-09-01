<!-- kvist-contract-version: 1 -->
# Sandbox Runner Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `sandbox-runner`
- **Contract IDs:** `sandbox-runner.protocol/v1`,
  `sandbox-runner.cli/v1`
- **Owner:** standalone `sandbox-runner` crate
- **Consumers:** the Kvist engine and Linux operators

The component owns independent request validation and operating-system
enforcement. It does not authorize grants, transition tasks, promote output,
or mint Kvist evidence.

## Provided interfaces

The scaffold builds an executable named `kvist-sandbox-runner`. The executable
returns a nonzero unavailable result and does not claim successful probe,
request validation, or isolation.

The accepted target interface is the redefined
`kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` protocol described by
ADR 0004. That target interface is not implemented in this scaffold revision.

## Required interfaces

The production runner will require Linux namespace support and a verified
Bubblewrap executable. The scaffold requires only the Rust standard library
and receives no engine authority.

## Data and schemas

No request schema is implemented by the scaffold. The future canonical bounded
JSON schema remains version 1 and will be defined by the queued protocol work
without recognizing the retired version-one shape.

## Behavioral guarantees

The scaffold fails closed for every invocation and never reports a request as
enforced. A successful build is evidence only that the component package is
structurally present.

## Errors and failure semantics

Invocation writes a concise unavailable diagnostic to standard error and
returns a nonzero exit status. Unknown arguments do not activate fallback or
host execution.

## Security and authority

The runner is a separate trust boundary. Future implementation must validate
all grants independently and must be installed as a regular non-link file
outside the selected project and worktree. The scaffold exercises no
filesystem, process, namespace, network, or credential authority beyond
writing its diagnostic.

## Compatibility and verification

Kvist is pre-release and retains no legacy protocol compatibility. Scaffold
tests cover only buildability and fail-closed unavailability. Native protocol,
Bubblewrap, path, resource, cleanup, and substitution tests are mandatory
before the target contract can be reported as implemented.
