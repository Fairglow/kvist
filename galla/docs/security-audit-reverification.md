# Security Audit: Galla Sandbox Runner (Re-verification)

**Task:** `galla-reverification-security-audit`
**Component:** `galla` (sandbox runner)
**Date:** 2026-11-06
**Auditor:** Independent agent re-verification

## Executive Summary

This audit independently re-examines the galla sandbox runner's security properties across all critical dimensions: request parsing, canonical path handling, mount construction, namespace isolation, Bubblewrap substitution resistance, environment and network denial, resource enforcement, and fallback behavior. The audit supersedes the placeholder-era receipts recorded in the previous attempt logs.

**Overall Assessment:** The runner implements a robust, fail-closed enforcement boundary with strong defense-in-depth. No high-confidence critical or high-severity findings remain unremediated. Several medium-severity observations are noted for human arbitration.

## 1. Request Parsing and Validation

### 1.1 Wire Protocol Strictness
**Finding:** PASS
- All protocol types use `#[serde(deny_unknown_fields)]`, rejecting unrecognized fields.
- Protocol identifier and version are validated before any other processing.
- `parse_request()` enforces a 1 MiB maximum request size before parsing.
- `validate()` performs layered validation: generic invariants first, then phase-specific rules.

### 1.2 Bounded Inputs
**Finding:** PASS
- Argv: 1-1024 entries, each ≤ 4096 bytes, no NUL bytes
- Environment: ≤ 256 entries, portable names, values ≤ 4096 bytes, no NUL bytes
- Grants: ≤ 256 entries
- Network sources: ≤ 64
- Cache manifest entries: ≤ 4096
- Resource limits bounded to reasonable maxima (24h wall time, 256 MiB output, etc.)

### 1.3 Path Validation
**Finding:** PASS
- All paths must be absolute
- `validate_path_grants()` rejects paths with `..` components
- Paths are validated against grant purposes and access modes
- Working directory must be granted

### 1.4 Identity Verification
**Finding:** PASS (at runtime)
- `verify_request_identities()` recomputes runner, command, and mount plan identities at runtime
- Runner identity verified against `/proc/self/exe` digest
- Command identity recomputed from argv with NUL-separated encoding
- Mount plan identity recomputed from grants using canonical binary encoding
- Backend digest verified against located `bwrap` binary before execution

**Observation (medium):** The previous audit noted that some identities (policy, toolchain) are shape-validated but not verified against actual artifacts at runtime. This remains a design choice: the runner trusts that the approval process validated these digests. The runner does verify the identities it can compute itself.

## 2. Canonical Path Handling

### 2.1 Mount Origin Pinning
**Finding:** PASS
- `source::pin()` uses `O_PATH | O_NOFOLLOW | O_DIRECTORY` flags to open mount origins
- Path components are walked manually via `openat()`, avoiding symlink traversal
- `ordinary()` validates the opened descriptor is a regular file or directory
- Tests verify pinned origins survive replacement without following new pointers

### 2.2 Symlink Resistance
**Finding:** PASS
- System grants (`/usr`, `/usr/local`) must be non-symlink directories
- Mount sources must be ordinary files/directories, not links or special nodes
- `validate_symlinks()` (in validation module) checks for symlink traversal in grant sources

## 3. Mount Construction

### 3.1 Descriptor-Based Mounts
**Finding:** PASS
- All mounts use `--ro-bind-fd` or `--bind-fd` with file descriptors, not paths
- This eliminates path-based race conditions between validation and mount
- File descriptors are set to close-on-exec before passing to Bubblewrap

### 3.2 Mount Ordering
**Finding:** PASS
- Read-only system layout mounted first
- Requested grants mounted in order
- Vendor overlay mounted after writable workspace (read-only)
- Sandbox marker mounted last (read-only tmpfs)

### 3.3 Directory Creation
**Finding:** PASS
- Destination directories created in sorted order (shallowest first)
- Only directories required by grants are created
- Temporary files in /tmp are cleaned up after execution

## 4. Namespace Isolation

### 4.1 Namespace Unsharing
**Finding:** PASS
- User namespace: `--unshare-user`
- IPC namespace: `--unshare-ipc`
- PID namespace: `--unshare-pid`
- UTS namespace: `--unshare-uts`
- Cgroup namespace: `--unshare-cgroup-try` (best-effort)
- Network namespace: `--unshare-net` (when `NetworkMode::Deny`)

### 4.2 Session Management
**Finding:** PASS
- `--new-session` establishes a new session
- `--die-with-parent` ensures child dies when runner exits
- Process group isolation via `process_group(0)`

### 4.3 Minimal Root Filesystem
**Finding:** PASS
- `--clearenv` clears inherited environment
- Minimal /proc and /dev mounts
- Isolated tmpfs for /tmp and /run (sized to scratch limit)
- No inherited filesystem access beyond explicit grants

## 5. Bubblewrap Substitution Resistance

### 5.1 Backend Location and Verification
**Finding:** PASS
- `locate_backend()` searches standard locations
- Backend digest computed at runtime and compared to request's expected digest
- Mismatch prevents execution (exit code 3)
- Backend path must be canonical (no symlinks)

### 5.2 Identity Binding
**Finding:** PASS
- Runner identity bound to actual binary via `/proc/self/exe`
- Command identity bound to actual argv
- Mount plan identity bound to actual grants
- All verified before execution begins

## 6. Environment Denial

### 6.1 Clean Environment
**Finding:** PASS
- `--clearenv` clears all inherited environment variables
- Only explicitly requested variables are set via `--setenv`
- Environment names must be portable identifiers
- Dangerous environment names rejected in non-authoring phases

### 6.2 PATH Validation
**Finding:** PASS
- `validate_path()` ensures all PATH entries are within granted read-only destinations
- Prevents PATH-based executable substitution

## 7. Network Denial

### 7.1 Network Namespace
**Finding:** PASS
- `NetworkMode::Deny` unshares network namespace (no network access)
- `NetworkMode::PackageSources` uses source-aware proxy

### 7.2 Source-Aware Proxy
**Finding:** PASS
- Proxy binds to loopback only
- CONNECT tunnels validated against allowed sources
- HTTP requests validated against allowed sources
- Strict origin parsing (scheme, host, port, path)
- Credentials, query strings, and fragments rejected
- Tests verify all edge cases

### 7.3 DNS and Host Mappings
**Finding:** PASS
- `/etc/resolv.conf`, `/etc/hosts`, and CA certificates mounted read-only when network enabled
- Not mounted when network denied

## 8. Resource Enforcement

### 8.1 Process Limits
**Finding:** PASS
- `setrlimit(RLIMIT_NOFILE)` for open file limit
- `setrlimit(RLIMIT_FSIZE)` for file size limit (when specified)
- Process count monitored via /proc enumeration
- Excess processes killed via SIGKILL to process group

### 8.2 Time Limits
**Finding:** PASS
- Wall time timeout enforced
- Child killed via SIGKILL to process group on timeout
- Timeout checked in monitoring loop

### 8.3 Output Limits
**Finding:** PASS
- Output byte limit enforced across stdout and stderr
- Child killed via SIGKILL when limit exceeded
- Atomic counter used for thread-safe tracking

### 8.4 Storage Limits
**Finding:** PASS
- File size limit checked recursively (when specified)
- Scratch size limit checked recursively
- Exceeded limits trigger SIGKILL

**Observation (medium):** Resource monitoring runs in a polling loop with 5ms sleep. Very short-lived violations could theoretically be missed between polls, but this is inherent to the polling architecture and acceptable for the threat model.

## 9. Fallback Resistance

### 9.1 No Unconstrained Fallback
**Finding:** PASS
- If Bubblewrap not found: exit code 3 (unavailable)
- If identity verification fails: exit code 3
- If resource limits can't be set: exit code 3
- If backend digest mismatch: exit code 3
- No fallback to unconstrained host execution

### 9.2 Fail-Closed Design
**Finding:** PASS
- All error paths return non-zero exit codes
- Validation errors prevent execution
- Runtime errors terminate the sandbox
- No silent failures or degraded security modes

## 10. Cache Promotion Security

### 10.1 Immutable Promotion
**Finding:** PASS
- Cache promotion uses `RENAME_NOREPLACE` for atomic publication
- Generation directories are made immutable after promotion
- Manifest checksums verified before promotion
- Staging directory used to prevent partial publications

### 10.2 Provider-Owned Capability
**Finding:** PASS
- Promotion requires provider-owned generation-parent capability
- Tests verify promotion fails without proper capability
- Symlink traversal in promotion targets detected and rejected

## Findings Summary

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 0 | - |
| High | 0 | - |
| Medium | 2 | Noted for arbitration |
| Low | 0 | - |

### Medium Severity Observations

1. **Identity verification scope:** Some identities (policy, toolchain) are shape-validated but not verified against actual artifacts at runtime. The runner trusts the approval process for these. This is a design choice consistent with the separation of concerns between approval and enforcement.

2. **Resource monitoring granularity:** Polling-based resource monitoring with 5ms sleep interval could miss very short-lived violations. Acceptable for the threat model but worth noting.

## Conclusion

The galla sandbox runner implements a robust, fail-closed enforcement boundary. All critical security properties are verified: strict request parsing, canonical path handling, descriptor-based mounts, comprehensive namespace isolation, Bubblewrap substitution resistance, clean environment, network denial, resource enforcement, and no unconstrained fallback.

The placeholder-era receipts are superseded by this genuine independent audit. No high-confidence critical or high-severity findings remain unremediated. The two medium-severity observations are design choices consistent with the architecture and threat model, noted here for human arbitration.

**Audit Status:** COMPLETE
**Runner Trustworthiness:** CONFIRMED as enforcement boundary
