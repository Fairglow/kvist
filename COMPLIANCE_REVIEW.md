# Phase 1 Compliance and Security Review

**Scope:** Core CLI Engine (`init`, `tree`, `spec new`, and `spec validate`).

## Independent review process

1. A clean-slate documenter inspected only Rust source and tests, then produced
   [`DOCS.md`](DOCS.md) without access to the architecture specification.
2. A separate compliance reviewer compared only
   [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
   and `DOCS.md`, without access to source code.

**Result:** The Phase 1 roadmap requirements are compliant: project
bootstrapping, deterministic component discovery/tree rendering, and layered
specification generation and validation are all present. Task execution, LLM
invocation, triple-blind pipelines, arbitration UI, web UI, and serving remain
explicitly deferred to Phases 2 and 3.

The architecture specification does not prescribe the exact CLI grammar, tree
status labels, overwrite policy, or detailed `SPEC.md` grammar. These
implemented contracts are documented in [`README.md`](README.md) and enforced
by tests.

## Security review

- No unsafe Rust, shared mutable state, network access, or background processes
  are used by Phase 1 commands.
- Generated files use same-directory temporary files, file synchronization, and
  no-clobber persistence. Existing and partial artifact sets are not
  overwritten.
- Project, configuration, component, and specification symlinks are rejected
  at direct checked paths; discovery does not traverse symlink entries.
- Configuration and specification parsing are bounded to 64 KiB and 1 MiB,
  respectively. Parsed component roots must be non-empty relative paths with
  normal path segments only.
- Filesystem, parsing, and validation failures are surfaced with contextual
  errors. CLI output is deterministic plain ASCII and does not emit secrets.

## Known operational limitation

Initialization writes each artifact atomically but is not a multi-file
transaction. If an I/O failure occurs after an earlier artifact is persisted,
the resulting partial Kvist artifact set is intentionally preserved and a
subsequent `init` refuses to merge or overwrite it; explicit user intervention
is required.
