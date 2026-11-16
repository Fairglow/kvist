# Security Audit: Settings Persistence and Authority Boundaries

**Audit ID:** workspace-settings-security-audit
**Date:** 2026-03-22
**Scope:** `maerg/src/model_settings/` module and related CLI integration

## Findings

### 1. Settings Persistence Mechanism

Settings are persisted across four distinct scopes with clear precedence:

| Scope | Path | Priority |
|-------|------|----------|
| Project | `kvist.toml`, `.kvist/config.toml` | Highest |
| User | `~/.config/kvist/config.toml` (platform-specific) | Medium |
| System | `/etc/kvist/config.toml` | Low |
| Built-in | Compiled defaults | Lowest |

**Audit Finding:** Scope separation is correctly implemented. Project settings override user settings, which override system settings. This provides appropriate authority boundaries for multi-user environments.

### 2. Mutation Safety

The `mutate_registry()` and `remove_local_profile()` functions implement several safety mechanisms:

- **Revision Pinning:** Mutations require an `expected_revision` parameter that must match the current source revision. Stale edits are rejected.
- **File Locking:** `SourceLock` serializes concurrent writers to the same configuration file.
- **Atomic Writes:** `replace_file_atomically()` ensures readers never see partial writes.
- **Pre-mutation Validation:** Role references are checked before disabling/removing profiles to prevent orphaned bindings.
- **Post-mutation Re-inspection:** After writing, the system re-inspects to detect concurrent changes that occurred during the write window.

**Audit Finding:** The mutation safety mechanisms are robust. The combination of revision pinning, locking, atomic writes, and re-inspection provides strong protection against data races and stale edits.

### 3. Authority Boundaries

**Inspection vs. Execution:** The `inspect()` function is strictly read-only. It never probes endpoints, executes commands, or authorizes execution. This is correctly documented in the code comments.

**Role Bindings vs. Profile Definitions:** Role bindings (which profile a role uses) are separate from profile definitions (the profile configuration itself). Changing a profile definition does not automatically change which roles use it.

**Shared Profile Effects:** When a user modifies a shared/user registry, the system warns that "Unknown external consumers of shared/user registries cannot be enumerated; this source change may affect them." This is an appropriate disclosure of the shared-state risk.

**Audit Finding:** Authority boundaries are clearly defined and correctly implemented. Inspection does not grant execution authority, and profile changes do not silently alter role bindings.

### 4. Input Validation

- TOML parsing errors are caught and reported with clear error messages
- Profile names are validated via `sav::registry::validate_profile_name()`
- Path safety is enforced via `safe_path()` to prevent directory traversal
- Configuration content validation is performed before and after mutations

**Audit Finding:** Input validation is comprehensive. All user-supplied values (profile names, paths, revisions) are validated before use.

### 5. Error Handling

- Uses idiomatic `Result<T, E>` throughout
- Specific error messages for each failure mode (stale revision, missing profile, invalid TOML, etc.)
- No use of `unwrap()` or `expect()` in production code paths

**Audit Finding:** Error handling follows Rust best practices. Errors are specific and actionable.

## Potential Improvements

1. **Logging:** Consider adding audit logging for settings mutations to track who changed what and when.
2. **Versioning:** Configuration file versioning could help with migration and compatibility.
3. **Backup:** Automatic backup of previous configuration versions before mutation could aid recovery.

## Conclusion

The settings persistence and authority boundary mechanisms in the `model_settings` module are well-designed and correctly implemented. The separation of inspection from execution, the use of revision pinning for mutations, and the clear scope hierarchy provide a solid foundation for secure configuration management.

No critical security issues were identified. The module correctly maintains the principle that inspection never grants execution authority, and mutations require explicit acknowledgment of the current state.
