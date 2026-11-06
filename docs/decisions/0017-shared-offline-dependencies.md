# ADR 0017: Shared offline dependency provisioning

## Status

Accepted direction by the human request on 2026-11-06. Rust is the first
consolidated backend; later language environments remain explicitly deferred.
Implementation and independent evidence belong in component queues and records.

## Decision

Extract package-management mechanics from maerg into the headless
`kvist-dependencies` library at `dependencies/`. Kvist and Skott are consumers,
not subprocess wrappers around one another. Sav supplies bounded process
supervision; Galla remains the isolation boundary. This separate component keeps
dependency provisioning distinct from model/provider mechanisms and task policy.

Operator `skott --vendor` explicitly provisions locked Rust dependencies on the
host; `--check` inspects readiness without effects and `--offline` limits Cargo
to cached material. No model configuration, Kvist project artifacts or UI is
needed. The command discloses host/network authority and is not a model tool.
No sandbox failure authorizes a host fallback. Protected tasks retain their
explicit unsupported-fulfillment refusal.

Keep common `.kvist` storage and the existing sandbox destinations/masking.
Publication/readiness and safe source replacement are shared. Skott retains its
private immutable vendor snapshot and network-denied execution. Each future
language needs both a provisioning backend and tested offline resource wiring;
package availability is not equivalent to executable offline support.

## Consequences

Rust mechanics have one provider-owned home; no maerg dependency enters Skott
and no task approval enters the library. Existing other-language maerg behavior
remains while future consolidation proceeds language by language.
Host acquisition can execute package-manager/configuration behavior and is an
explicit operator capability, not sandboxed execution or approved model
authority. Snapshot refresh initially requires restarting Skott; no prompt or
effect is automatically replayed. Lock resolution and toolchain installation
remain separate provisioning operations.
