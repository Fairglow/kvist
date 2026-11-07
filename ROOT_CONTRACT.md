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
  Explicitly approved Rust workspace/path-dependency build resources may expose
  source read-only for authoring builds, without adding peer intent to prompts
  or any peer write authority (ADR 0014).
- Treat agent configuration, prompts, and output as untrusted input. External
  commands must be resolved without a shell and covered by an explicit,
  human-approved execution policy before they run. A one-off host command may
  use an explicit invocation acknowledgement that states its authority and
  scope; sandboxed task execution requires separately persisted policy
  approval.
- Keep authoring, dependency acquisition, verification, and promotion as
  distinct authority phases. Authoring receives only approved writable paths.
  Dependency acquisition may reach exact configured package sources and may
  write only attempt-local dependency and workspace state, and MAY provision
  versioned, enforced vendored registries on the host through shared
  dependency mechanics (ADR 0017, Rust first; application authority remains
  separate; ADR 0018 makes Rust share one tree, a minimal lock receipt and
  configurable common vendor limits, leaving file checksums to Cargo), a language-aware model:
  Rust enforced exactly through Cargo.lock; Python/uv, Node/npm, and Conan
  enforced through their respective lock files. Verification runs without
  network access and builds offline from that vendored registry. Agents must not
  write queues, intent, implementation records, approval state, or canonical
  evidence.
- Protected maerg verification uses its approved `--offline --locked` policy.
  Skott workspace Cargo remains offline but may resolve the writable project
  lock. Live installed tools and available vendored sources do not require a
  frozen lock digest, application receipt or session restart (ADR 0019).
  Acquisition remains separate from network-denied workspace execution.
- Workspace isolation protects the host from unapproved sandbox effects, not
  the sandbox from host edits. Read-only mounts are live inputs; legitimate
  external changes must not be classified as breaches or global tool blockers.
  No general file-size quota is imposed on workspace files. Use bounded streaming,
  working memory, result pages and process budgets; never enforce payload limits
  by scanning or deleting operator-owned files. Exact mutation matching and
  optional explicit preconditions may prevent lost user edits without freezing
  ordinary reads. Privileged command names are not authority; OS confinement is.
- Prefer safe Rust. Unsafe is not categorically prohibited, but each use must
  be necessary, minimally scoped, and justified by documented safety
  invariants, targeted verification, and independent review. Existing
  crate-level unsafe guards must not be broadly relaxed to bypass this rule.
- Keep model networking and credentials outside the effect sandbox. Initial
  task execution may use local agents; future remote agents require a
  host-owned transport and typed tool broker rather than a mounted user home or
  raw provider credential.
- Support executable agent workflows on Linux only until independently tested
  platform backends exist. Keep operating-system enforcement behind explicit
  interfaces so deferred platforms do not leak conditional behavior through
  provider, policy, or workflow code.

## Change and compliance rules

- Before target-workflow acceptance, give each controlled local component
  intent bundle an advisory AI review opportunity or record an explicit
  exception. The bundle is `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and
  the canonical task-definition projection of `TODOS.yaml`; generated intent
  drafts are included even when an agent produced them.
- Treat review feedback as nonbinding input. Findings may be incorrect or
  overly exacting and never require a document change, block acceptance by
  severity, or determine compliance. When project review is required,
  acceptance requires review plus explicit human acknowledgement, or a
  deliberate exception bound to the exact bundle digests. A persistent project
  opt-out may disable the per-bundle gate but must be visible and explicit.
- Keep `component accept` deterministic and local. Current behavior only
  validates structure and records revisions; advisory-review enforcement and
  project-level acceptance are planned and must not be represented as already
  implemented.
- `TODOS.yaml` orders work as tests, implementation, security audit, then
  compliance review.
- Requirements state what must be achieved, contracts state what consumers may
  rely on, and designs state how a component intends to satisfy them. Do not
  duplicate normative facts across these artifacts.
- `IMPL.md` describes observed implementation behavior and is not copied from
  intended requirements, contracts, or designs. Human-facing user and
  integration documentation belongs under `/docs/`.
- Generated evidence—`IMPL.md`, compliance and review reports, status, and
  attempt logs—is exempt from the advisory document-review gate.
- A clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant.
- Record intent-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact.
- Do not treat external process success as human acceptance. Supervised
  execution records a reviewable attempt, and ambiguous interrupted attempts
  remain fenced until explicit recovery and disposition.
- A component run may execute dependency-ready tasks without per-task human
  scheduling. Successful execution is not human acceptance. Policy-controlled
  local item checkpoint commits may precede final component approval; they
  must preserve unrelated changes, never push, retain exact evidence and stop
  on decisions, ambiguous effects or commit failures.
- Keep acceptance distinct from VCS publication. An explicit commit option may
  create a local commit containing only the exact accepted paths and
  maerg-written evidence. It must preserve unrelated worktree and index state,
  never push, and retain accepted state for explicit recovery if commit
  creation fails.
