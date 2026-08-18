<!-- kvist-root-contract-version: 1 -->

# Kvist Root Contract

This contract applies to every component in this project. It is the global
constraint set injected into component work.

## Non-negotiable architecture

- Define and validate a component's specification, public contract,
  constraints, and test strategy before implementation.
- Keep each component's `SPEC.md`, `TODOS.yaml`, `DOCS.md` (which logically represents `IMPL.md` / the component's internal Implementation Compliance Record), and implementation adjacent in its directory.
- Persist architecture and workflow state in version-controlled project files.
- Keep component context limited to the component, its immediate parent
  contract, and this root contract.

## Change and compliance rules

- `TODOS.yaml` orders work as tests, implementation, security audit, then
  compliance review.
- `DOCS.md` (the logical `IMPL.md` record) describes observed implementation behavior and is not copied from
  `SPEC.md`. Human-facing user and integration documentation belongs separately under a dedicated `/docs/` structure.
- A clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant.
- Record specification-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact.
