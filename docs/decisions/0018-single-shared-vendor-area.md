# ADR 0018: One shared Rust vendor area and lightweight readiness

## Status

Superseded for default storage/resource selection by
[ADR 0020](0020-native-offline-dependency-caches.md). Native cache reuse removes
the routine project vendor tree; the no-inventory principle and explicit
portable export remain. The decision below is historical.

Human-directed change on 2026-11-07. Supersedes ADR 0017's private vendor-copy
and content-attestation choices, not its shared library or authority boundaries.

## Decision and rationale

Kvist and Skott use the same project-local `.kvist/vendored` directory.
Neither requires a second package tree. Skott mounts that tree read-only at
`/rust/vendor`; the existing runner mask makes its writable workspace alias
inaccessible. Small runtime wrappers and a source-only resolver may still be
staged outside the workspace. They are not a second vendoring area.

Readiness checks lock binding, publication fences, resolver semantics and
package/native-metadata presence. They do not traverse and rehash package
contents or produce a file-checksum inventory. Cargo's own
`.cargo-checksum.json` files remain because Cargo requires them; Cargo handles
its native validation when packages are actually consumed. We do not duplicate
that work or claim independent archive/Git provenance.

The project receipt records only its schema version and Cargo.lock digest.
Producer paths, timestamps and a misleading full-verification flag are omitted.
All vendor-specific quotas and process budgets have defaults and are configurable
in the optional shared `.kvist/vendor.toml`, independent of either application's
model configuration. Intrinsic filesystem/protocol restrictions are not vendor
quotas.

Readers hold a shared directory lock only while preparing/using a vendor view.
Publication takes an exclusive directory lock before replacing or retiring the
current tree; concurrent library writers retain the existing operation fence.
No reader catalogue, daemon, duplicated cache or persistent per-file state is
introduced. Idle Skott sessions do not block provisioning. A changed lock or
published directory requires an explicit session restart, never prompt replay.

## Tradeoffs

A lightweight readiness result is not proof that arbitrary source files are
uncorrupted or that a build will succeed. Direct host modification and package
managers outside this shared library do not honor its reader/writer protocol.
Use the explicit operator provisioning commands, not in-place concurrent edits.
Operator acquisition remains distinct from network-denied agent execution;
no host fallback, automatic downloads or toolchain installation is added.
