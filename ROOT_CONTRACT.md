<!-- kvist-root-contract-version: 1 -->

# Kvist Root Contract

This contract applies to every component in this project. It is the global
constraint set injected into component work.

## Non-negotiable architecture

- Approve `VISION.md` and `ARCHITECTURE.md` before detailed component work.
- Define and validate a component's requirements, consumer contract, design,
  constraints, acceptance criteria, and verification strategy before
  implementation.
- Keep each component's `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
  `TODOS.yaml`, `IMPL.md`, and implementation adjacent in its directory.
- Persist architecture and workflow state in version-controlled project files.
- Keep component context limited to local artifacts, explicitly required
  provider contracts, the immediate parent `CONTRACT.md`, and this root
  contract. Exclude peer and parent designs and implementations by default.
- Treat agent configuration, prompts, and output as untrusted input. External
  commands must be resolved without a shell and covered by an explicit,
  human-approved execution policy before they run.
- Support executable agent workflows on Linux only until independently tested
  platform backends exist. Keep operating-system enforcement behind explicit
  interfaces so deferred platforms do not leak conditional behavior through
  provider, policy, or workflow code.

## Change and compliance rules

- `TODOS.yaml` orders work as tests, implementation, security audit, then
  compliance review.
- Requirements state what must be achieved, contracts state what consumers may
  rely on, and designs state how a component intends to satisfy them. Do not
  duplicate normative facts across these artifacts.
- `IMPL.md` describes observed implementation behavior and is not copied from
  intended requirements, contracts, or designs. Human-facing user and
  integration documentation belongs under `/docs/`.
- A clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant.
- Record intent-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact.
