# Security Audit: Native Bubblewrap E2E Test Authority

**Task:** `enable-native-e2e-security-audit`
**Component:** `galla` (sandbox runner)
**Date:** 2026-11-05
**Superseded by:** `security-audit-reverification.md` (2026-11-06) — genuine independent re-verification

## Executive Summary

This audit examines the security implications of enabling the native Bubblewrap
e2e tests in the `galla` component. The tests provide enforcement evidence that
the sandbox runner correctly isolates processes, mounts filesystems, and
enforces resource limits. However, enabling them introduces several security
considerations that must be understood.

## Test Authority and Trust Assumptions

### Runner Location and Identity

The e2e tests establish trust in the runner through:

1. **Explicit override:** `KVIST_GALLA` environment variable points to an
   installed runner outside the worktree (CI use case).
2. **Cargo integration:** `CARGO_BIN_EXE_galla` provides the path to the
   freshly built binary (local test use case).
3. **Fallback:** The tests locate the built binary in the target directory.

**Security property:** The runner must be outside the selected VCS worktree.
Tests explicitly verify this:

```rust
assert!(
    !runner.starts_with(repository),
    "runner must be outside the worktree"
);
```

**Risk:** If the target directory is inside the worktree, the fallback
location could be compromised. CI environments use `KVIST_GALLA` to avoid
this.

### Backend Identity and Digest Verification

The probe establishes backend identity:

1. **Path canonicalization:** The probe reports the canonical path of the
   backend, not a symlink target.
2. **Digest verification:** The probe computes and reports the SHA-256 digest
   of the backend binary.
3. **Symlink detection:** Tests verify the reported path is not a symlink.

**Security property:** The backend digest is verified at runtime against the
located `bwrap` binary. This prevents substitution attacks if the approval
digest is known.

**Risk:** `locate_backend` follows `PATH` and uses `is_file()` (which follows
symlinks). A malicious `bwrap` could be substituted if `PATH` or
`/usr/bin/bwrap` is attacker-controlled, mitigated only by an approval digest.

### Identity Validation vs. Verification

The tests validate that identities have the correct digest shape
(`sha256:` prefix with 64 hex characters) but do not verify that the digest
matches the actual artifact at runtime. This is a gap identified in the
previous audit attempt.

**Verified identities:**
- Backend digest: verified against the located `bwrap` binary

**Shape-validated only (not verified at runtime):**
- Runner identity
- Policy identity
- Command identity
- Mount plan identity
- Toolchain identity

**Security implication:** An attacker could substitute the runner, policy, or
other artifacts as long as they provide a valid-format digest. The approval
process must ensure the digests are correct before execution.

## Environment Constraints

### Sandbox Detection and Nested Sandbox Protection

Tests self-skip when already inside a Galla sandbox:

```rust
if std::fs::symlink_metadata(galla::context::SANDBOX_MARKER).is_ok() {
    eprintln!("skipping: already inside a Galla sandbox");
    return;
}
```

**Security property:** The `.galla-sandbox` marker file prevents nested
sandbox execution, which would fail closed before establishing enforcement
evidence.

**Risk:** If the marker is absent or corrupted, tests could run inside an
existing sandbox, producing misleading results.

### Toolchain Detection and Skip Behavior

Tests self-skip when required toolchains are not available:

- `bubblewrap` (required for all e2e tests)
- `python3`, `node`, `go`, `gcc`, `g++` (for system toolchain tests)
- `cargo`, `rustc`, `rustup` (for Rust-specific tests)

**Security property:** Tests never fail in an environment that lacks the
required toolchains; they self-skip with diagnostic output.

**Risk:** Silent skips in CI could mask test failures if toolchain
installation is not verified separately.

### Network Isolation Tests

Network isolation tests use loopback addresses and local TCP listeners:

```rust
let listener = TcpListener::bind("127.0.0.1:0").unwrap();
```

**Security property:** Tests verify network isolation without requiring
external network access.

**Risk:** Tests assume loopback addresses are available and not blocked by
firewall rules. In some containerized environments, loopback may be restricted.

## Enforcement Evidence Validity

### Mount Verification

Tests verify mount behavior through:

1. **Read-only mounts:** Write attempts to read-only mounts should fail.
2. **Writable mounts:** Write attempts to writable mounts should succeed.
3. **Symlink resolution:** Tests verify symlinks resolve to the correct paths.

**Security property:** Mount enforcement is verified through actual filesystem
operations, not just inspection of mount options.

**Risk:** Tests do not verify recursive symlink escapes. A symlink in a
read-only mount could point to a writable location.

### Resource Limit Enforcement

Tests verify resource limits through:

1. **File size limits:** Write attempts exceeding the limit should fail.
2. **Process count limits:** Spawn attempts exceeding the limit should fail.
3. **Output limits:** Output exceeding the limit should be truncated.
4. **Wall time limits:** Execution exceeding the limit should be killed.

**Security property:** Resource limits are enforced through actual limit
breaches, not just inspection of limit settings.

**Risk:** Tests use `prlimit` for limit enforcement, which may not be
available in all environments. The `safe_prlimit` function ignores errors,
which could mask limit enforcement failures.

### Network Isolation Enforcement

Tests verify network isolation through:

1. **Deny mode:** Network access should be blocked.
2. **PackageSources mode:** Network access should be limited to allowed
   sources.
3. **DNS resolution:** Tests verify DNS resolution is pinned to public
   addresses.

**Security property:** Network isolation is verified through actual network
access attempts, not just inspection of network namespace settings.

**Risk:** The proxy does not pin resolved addresses. Host DNS compromise or
rebinding can point an allowed hostname to a private or loopback IP.

## Security Gaps Identified

### 1. Enforcement Promotion is Not Immutable

`inspect_and_promote_cache` walks the host source and copies into
`project-cache`. It does not use atomic rename, staging, no-follow
descriptors, immutable modes, manifest checksums, generation identity, or
lockfile/source identities.

**Impact:** Cache promotion is non-atomic and mutable. Concurrent access could
corrupt the cache.

### 2. Network Guard is Advisory for PackageSources

`run` adds `--unshare-net` only for `Deny` mode. In `PackageSources` mode, the
child shares the host network namespace and can ignore `http_proxy`.

**Impact:** The proxy enforces only clients that honor the environment. A
malicious client could bypass the proxy.

### 3. CONNECT Authorization Bypasses Path Restrictions

`OriginPolicy::authorizes_authority` matches scheme/host/port only. A CONNECT
to an allowed host is accepted even when the allowed origin is a subpath.

**Impact:** Any path on an allowed host can be tunneled over TLS, bypassing
path restrictions.

### 4. Proxy Does Not Pin Resolved Addresses

The proxy connects to the parsed host without checking DNS results. Host DNS
compromise or rebinding can point an allowed hostname to a private or loopback
IP.

**Impact:** DNS-based attacks can bypass network isolation.

### 5. Schema/Conformance Mismatch

`tests/conformance.rs` asserts the purpose enum equals seven values, omitting
`registry`, `cargo-config`, and `runtime` that `protocol.rs` accepts.

**Impact:** The schema is incomplete relative to the parser. Tests pass
because they only check for the presence of values, not the absence of others.

## Recommendations

### High Priority

1. **Verify identities at runtime:** Compute and compare digests for runner,
   policy, command, and mount plan identities, not just shape validation.
2. **Pin DNS resolution:** Use `authorizes_resolved` to check DNS results
   against the policy.
3. **Fix schema conformance:** Update the conformance test to check for exact
   enum values, not just presence.

### Medium Priority

4. **Atomic cache promotion:** Use staging and atomic rename for cache
   promotion.
5. **Network namespace for PackageSources:** Add `--unshare-net` for
   `PackageSources` mode and use a dedicated network namespace for the proxy.
6. **Path restrictions for CONNECT:** Authorize CONNECT requests on the full
   URL, not just the authority.

### Low Priority

7. **Improve probe accuracy:** Execute `bwrap` to test namespace creation
   rather than relying on `/proc/self/ns` existence.
8. **Recursive symlink validation:** Check for recursive symlink escapes in
   writable grants.
9. **fsync for immutable mode:** Add fsync after `fchmod` for immutable mode
   durability.

## Conclusion

The native Bubblewrap e2e tests provide valuable enforcement evidence for the
sandbox runner. However, several security gaps remain that could be exploited
in a hostile environment. The tests themselves are well-designed and provide
strong evidence of correct behavior when the environment is trusted.

The primary risk is that the tests validate enforcement under ideal conditions
but do not test adversarial scenarios such as DNS rebinding, symlink escapes,
or proxy bypass. These scenarios should be tested separately or in a dedicated
security test suite.

Enabling the tests is recommended with the understanding that they provide
evidence of correct behavior under normal conditions but do not guarantee
security against all attack vectors.
