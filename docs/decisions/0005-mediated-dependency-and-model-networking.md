# ADR 0005: Mediate dependency and model networking

## Status

Proposed

## Context

Permanent network denial prevents Cargo from resolving a newly added
dependency or selecting a newer compatible version. Unrestricted network
access, however, lets arbitrary agent-controlled processes exfiltrate data,
reach undeclared services, mutate remote state, or run dependency build
scripts with ambient authority. Remote model agents also need network access
and credentials, but those capabilities are different from package
acquisition and should not share one broad sandbox permission.

## Decision

Keep authoring and verification sandboxes network-denied. Add a distinct
host-authorized dependency-acquisition phase that runs Cargo with network
access limited to configured supported sources.

The initial supported source is crates.io through its canonical sparse-index
and crate-download locations. Additional registries require an explicit
project policy containing canonical index and download origins. Git
dependencies require an exact approved repository URL and immutable revision;
moving branches, URL rewrites, credential helpers, and ambient Cargo source
replacement are rejected initially.

Cargo receives an attempt-local writable `CARGO_HOME`, registry source/cache,
Git database, lockfile workspace, and target scratch directory. It does not
receive the user's Cargo home. Successful bounded acquisition may promote a
content-addressed project cache only after checksum, lockfile, source-policy,
size, file-count, path, and link validation. Verification then runs with
network denied, `--locked`, and only the approved cache mounted read-only.
Dependency build scripts never run in the network-enabled acquisition phase.

The policy records supported source identities rather than a loose list of IP
addresses. Name resolution and CDN address changes are handled by an
application-aware fetch boundary; they do not broaden the approved origin set.

The initial task agent is local and receives no model network or credentials.
Future remote models run through a host-owned model transport and typed tool
broker outside the effect sandbox. The broker holds credential references,
limits destinations and operations, and submits explicitly authorized tool
requests to the runner. Mounting a provider CLI's home directory or raw token
into the sandbox is not an accepted remote-agent design.

## Alternatives considered

- **Deny all network permanently:** strongest simple boundary, but prevents
  normal dependency evolution.
- **Give the complete task sandbox unrestricted network:** convenient but
  defeats the authority and confidentiality model.
- **Allow network only to the `cargo` executable in the same sandbox:** process
  identity is not sufficient mediation when an agent can invoke Cargo or its
  subprocesses with attacker-controlled inputs.
- **Allowlist IP addresses:** brittle for registries and CDNs and does not
  express application-level read-only operations.
- **Mount the user's Cargo cache writable:** risks corruption, credential
  disclosure, source substitution, and cross-project contamination.

## Consequences

Dependency changes require an explicit acquisition step and may need human
approval for a new source. Initial implementation is more complex than
unrestricted egress, but normal crates.io dependency additions remain possible
without giving the authoring agent general network authority. Remote agents
remain a later brokered capability rather than an exception to sandbox policy.
