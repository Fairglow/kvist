# ADR 0020: Native offline dependency caches

## Status

Accepted design by explicit human direction on 2026-11-07. Supersedes the Rust
directory-vendor default in ADRs 0011, 0017 and 0018; ADR 0019's live-resource,
one-way isolation remains authoritative. Implementation and independent evidence
are recorded separately in component queues and records.

## Context

Re-downloading into an empty Cargo home, copying all dependencies with
`cargo vendor`, and synchronizing every copied file make ordinary preparation
unnecessarily expensive. Already downloaded archives and expanded sources can
serve native Cargo offline without becoming a directory source. A native cache
is not a portable, version-controlled vendor export.

## Decision

Rust preparation reuses the operator-selected native Cargo cache. Exact locked
metadata and bounded package/source availability checks replace routine source
copying, recursive content walks and application file-checksum inventories.
Acquisition is explicit, incremental and non-compiling (`cargo fetch --locked`);
missing downloads and extraction are useful work, not fast-path overhead.
Completed cached material survives a failed or interrupted acquisition.

Authoring and verification deny networking and mount approved registry and Git
data read-only. They do not mount a user's complete Cargo home, configuration,
credentials or installed binaries. Cargo has private writable administrative
state for locks and cache-use metadata. Protected verification is
`cargo test --offline --locked`; workspace authoring remains offline but may
resolve its writable lock. Native inputs remain live: host removal or corruption
requires a clear cache/resource error, not an immutable-content claim or automatic
network fallback. Acquisition writers must coordinate applicable native locks;
private sandbox administrative locks alone do not pin a host cache against GC.

Readiness records contain small lock-bound metadata, never checksums of all
source files. Cargo owns archive verification and native source semantics.
Directory vendoring remains a deliberately selected export capability; no
fabricated `.cargo-checksum.json`, symlink vendor farm or hard-linked source
duplicate is introduced.

The same principle applies to other supported backends, not the same layout.
JavaScript stores, Python wheels/environments, Go modules and Conan/vcpkg
packages retain their package-manager-native semantics. Installing a virtual
environment, `node_modules`, generated build files or target-specific binaries
can require useful materialization. Existing catalogue-only readiness must not
be described as complete package integrity or exact origin attestation.

Transport is full-duplex, concurrent and bounded. Acquisition failures preserve
the actual cause and identify phase, timeout/lock conditions and safe recovery.
Persistent native caches use process-held locks rather than stale pathname
existence as liveness. No existing operator interruption fence is stolen or
removed when selecting the new native path.

## Performance and consequences

The target is preparation below 30 seconds for a warm bounded graph on defined
hardware, with no dependency source copies or application per-file hashing.
Cold networking, extraction, required installation and compilation are excluded.
Native Cargo-home internals require compatibility tests for supported Cargo
versions. Portable exports cost more deliberately. Approval/protected task
authority and independent source-derived compliance evidence are unchanged.

## Alternatives

Repeated full vendoring preserves a portable export but duplicates data and
work. Fake directory-source metadata bypasses required Cargo semantics.
Hardlinks share mutable inodes; reflinks still require per-file metadata work
and filesystem-specific fallback. None is the default warm path.
