# ADR 0019: One-way workspace isolation and live host resources

## Status

User-directed decision, 2026-11-07. Supersedes the workspace-agent freeze,
file-size and runtime-readiness aspects of ADRs 0011, 0012, 0017 and 0018.
Protected maerg approval, provenance and evidence remain separate workflow
obligations, not requirements imposed on every workspace tool.

## Decision

The sandbox protects the host from unapproved effects by its payload. It does
not protect the payload from the operator. Host edits, replacements, upgrades
and provisioning are legitimate inputs, not contract violations. Read-only
resource mounts are live views; subsequent tools refresh changed mount topology
and generated selection/source configuration without a restart requirement.

There is no general file-size quota for workspace inspection, mutation or
process output files. Native operations stream files using bounded working
memory and bounded result pages. Search work is resumable when its per-call
budget is reached, including within one large file. Explicitly requested
preconditions and exact edit/patch matching prevent accidental lost updates;
ordinary reads do not implicitly freeze revisions.

Workspace Rust execution uses installed read-only tools and currently available
vendored sources, without a mandatory application receipt, frozen lock digest
or reader lease. Missing dependencies are diagnosed by the affected build,
not used to hide otherwise useful Rust tools. Cargo's native source metadata
and language semantics are not sandbox security guarantees. Workspace Cargo is
offline but may resolve its writable project lock; protected verification keeps
its explicitly approved locked semantics.

Administrative executable directory names and shell substrings are not
authority boundaries. Standard system executables are read-only; the operator
may add command filters, but normal cleanup and inspection are not categorically
forbidden. Namespace/capability restrictions, disjoint grants, private scratch,
unexposed credentials and network policy remain enforced.

Resource enforcement acts on the sandbox process, never by scanning and deleting
operator-owned files. Optional file-write caps may be explicitly requested by
other consumers; workspace tools request none. Bounded protocol/model payloads,
working memory, output capture, process/open-descriptor budgets and cancellation
remain justified protection against host exhaustion and unusable responses.

## Consequences

Host changes can produce normal compiler, missing-path or stale-edit errors.
Report the affected operation, actual cause, preserved work and a concrete
recovery, while continuing safe independent work. Do not invent an integrity
breach, silently replay effects or claim atomic snapshots of concurrent files.
Native library-specific checksum errors remain actionable build errors, not
permission to rewrite a read-only source tree.

This human-directed intent change has an explicit advisory exception while
project/bundle receipt enforcement is unavailable. Independent implementation
observation and source-blind comparison remain separate from acceptance.
