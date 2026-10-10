# ADR 0014: Read-only Rust authoring build context

## Status

Approved by the human for this repair on 2026-10-10: automatically mount declared
workspace dependencies and Cargo metadata read-only. This is not component
acceptance or independent compliance certification.

## Context

Installed Rust tools alone cannot build a component that inherits workspace
package/dependency fields or depends on sibling crates. The component's fixed
`/workspace/component` namespace does not preserve those relative Cargo paths.
Mounting the project writable would violate protected artifacts and peer scope.

## Decision

Prepare a separately validated, read-only Cargo build view at `/rust/project`.
Preserve original workspace metadata, lockfiles, declared workspace members and
project-local path dependencies. Source/assets are selected without provider
intent/state, hidden operational data, generated caches or user homes.
Reject outside-project roots, links, unsupported member globs and exceeded
bounds explicitly. Normal PATH Cargo runs in the component's build-view path;
compiler/formatter source arguments map back to `/workspace/component` so
authorized source writes remain in their existing scope.

This resource access is explicitly approved build context, not implicit prompt
context. No provider write grants, networking, package installation or broader
verification/cache authority are added. Direct concrete Cargo paths remain
outside the convenience wrapper.

## Consequences and evidence

Multi-crate offline tests must exercise workspace inheritance and path
dependencies, immutable provider source and root metadata, host lock preservation
and malformed/escaping declarations. Toolchain/default inventory coverage
remains separate. Providers or assets not permitted by these bounds fail
explicitly; they are not silently fetched or granted broader authority.
