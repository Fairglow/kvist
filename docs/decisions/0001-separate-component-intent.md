# ADR 0001: Separate component requirements, contract, and design

## Status

Accepted

## Context

The original `SPEC.md` combined component outcomes, public behavior, quality
constraints, algorithms, state transitions, and failure handling. That forced
consumers and child components to receive internal design information and made
an unrelated parent design change look like a public contract change.

Kvist needs distinct authority, bounded agent context, attributable staleness,
and independent compliance comparison.

## Decision

Every component owns adjacent `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`.

- Requirements define testable outcomes, quality scenarios, constraints, and
  acceptance.
- The contract defines only provided and required consumer-visible behavior.
- The design defines the private strategy for satisfying approved requirements
  and contract.

`SPEC.md` is retired. The task queue records independent revisions for all
three local documents and the immediate parent contract. Consumer contracts
may reference optional native machine-readable schemas by exact path and
dialect/version.

## Alternatives considered

- Retain one progressively disclosed specification: fewer files, but it does
  not enforce context or change-propagation boundaries.
- Rename `SPEC.md` to `DESIGN.md`: clearer for algorithms, but loses a distinct
  home for requirements and consumer behavior.
- Use `INTERFACE.md`: too narrow because consumers also depend on semantic,
  failure, security, and compatibility guarantees.
- Use only external IDLs: useful for syntax, but insufficient for general
  component behavior and unavailable for many internal or CLI boundaries.

## Consequences

Component setup and validation handle three intent documents. Discovery
requires five adjacent artifacts including the queue and implementation record.
Local changes have distinct staleness causes, and only the parent contract is
an implicit upstream revision. More files are maintained, but each has a
smaller audience and a single authoritative purpose.
