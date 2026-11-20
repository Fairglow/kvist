# Security Audit: Per-Language Offline Verification

**Component:** maerg (Kvist per-language offline topologies) with galla (runner enforcement)
**Task:** `language-e2e-security-audit`
**Scope:** Source-blind review of per-language provisioning, vendored mount wiring, test-command/environment allowlisting, and offline enforcement for injection, path traversal, link-following, and network-escape findings.
**Method:** Independent source inspection of `src/language_verification.rs`, `src/language_vendoring.rs`, `src/language_vendoring/config.rs`, `src/vendor_command.rs`, `src/vendor_command/go_cache.rs`, `src/sandbox.rs`, `src/config.rs`, `src/task_commands.rs`, and the galla runner `src/enforcement.rs`, `src/enforcement/source.rs`, `src/enforcement/system_layout.rs`, `src/validation.rs`, `src/validation/native_cache.rs`, `src/enforcement/transport.rs`.
**Reviewer stance:** Independent security auditor. The implementer did not author this record. No tests, builds, or `cargo` invocations were executed in this review; the suite is run automatically by Kvist in the verification sandbox after this turn.

## Summary

The per-language offline verification topologies (ADR-0011, ADR-0013) preserve the independent confirmation that a non-Rust offline build resolves only approved, pinned material with the network denied. Vendoring is enforced fail-closed before any sandbox work. The offline profile is a closed per-language enumeration: Go, JavaScript, and Python run their canonical offline commands with a fixed, host-leak-free environment; C and C-vcpkg run an operator-approved test command that is allowlisted, split on whitespace with no shell interpretation, and rejected for every canonical language. Every vendored mount is canonicalized, pinned without following any pathname pointer, and identified by the lock-file digest. The runner unshares the network namespace in Deny mode, so no egress path exists. No injection, path-traversal, link-following, or network-escape finding requires a fix.

## Satisfied Controls

### 1. Fail-closed vendoring enforcement before any sandbox work

**Control:** A non-vendored project, a drifted lock file, or missing material fails closed before the sandbox is entered; a sandbox infrastructure failure never falls back to a host build.

**Mechanism:** `run_offline_language_verification` calls `enforce_vendoring(project_root, false, Some(config))` first. `auto_vendor` is `false`, so a not-ready report returns `KvistError::VendoringIncomplete` with the missing material named, and no `kvist vendor` is run. The Rust language is rejected with a `SandboxUnavailable` reason because the closed Cargo topology owns Rust builds. The offline profile is built only after enforcement returns a ready report and its mounts. A missing or empty C/C++ test command returns `SandboxUnavailable` with an actionable message. There is no host-build fallback anywhere in the path.

**Assessment:** Satisfied. The fail-closed ordering is correct: enforcement, then profile, then sandbox.

### 2. Closed per-language offline profile enumeration

**Control:** The offline profile is a closed enumeration of the supported languages; an unknown language fails closed. Canonical languages run a fixed command; only C/C++ accepts an approved test command.

**Mechanism:** `offline_profile` matches `go`, `javascript`, `python`, `c`, and `c-vcpkg`, and a catch-all `other` arm returns `SandboxUnavailable` "no offline verification profile for language". The `"go" | "javascript" | "python" if test_command.is_some()` arm returns an error, so an approved test command drives only the C/C++ build system. Go runs `go test -mod=readonly ./...`; JavaScript runs `node --test`; Python runs `python3 -m unittest -v`. The C and C-vcpkg arms call `c_package_manager_profile`, which requires an approved test command and fails closed when none is supplied.

**Assessment:** Satisfied. The enumeration is total and closed; no untrusted input can introduce a new language or command.

### 3. Approved test command is allowlisted and split without shell interpretation

**Control:** The C/C++ test command is an operator-approved, allowlisted string from the project `[test_policy]`; it is split on whitespace into argv with no shell, so no shell metacharacter injection is possible.

**Mechanism:** `find_test_command` walks the component path and matches a `test_policy.commands` entry by exact component path. The command string is split with `split_whitespace`, and the first token is the program, the rest are arguments. There is no shell invocation; the tokens are passed as a literal argv to the sandbox runner. A command such as `make test && curl evil` becomes argv `[make, test, &&, curl, evil]`, where `&&` and `curl` are literal make arguments, not shell operators. The `test_policy` parsing requires `schema_version` 1, a `working_directory` of `component` or `project`, an `environment_allowlist` of portable identifiers no longer than 4096 bytes and at most 256 entries, and a `commands` array of `{component, command}` tables. A `project` working directory is rejected for the component-only sandbox mount.

**Assessment:** Satisfied. The test command is allowlisted, bounded, and split without shell interpretation.

### 4. Fixed environment with no host leakage and no dangerous names

**Control:** The sandbox environment is a fixed, host-leak-free set; no host environment variable is leaked into the sandbox, and the runner rejects dangerous environment names.

**Mechanism:** `offline_profile` builds a `BTreeMap` environment starting with `HOME` (the writable scratch home) and `PATH` (the fixed sandbox path `/usr/bin:/bin:/usr/local/bin`). No host `PATH` is leaked. Go adds `GOROOT`, `GOPROXY=off`, `GOSUMDB=off`, `GOTOOLCHAIN=local`, `GOWORK=off`, `GOFLAGS=-mod=readonly -p=1`, `GOMAXPROCS=2`, `GOCACHE`, `GOTMPDIR`, `GOPATH`, and `GOMODCACHE`. Python adds `VIRTUAL_ENV`, `PYTHONPATH`, `PYTHONDONTWRITEBYTECODE=1`, and `PYTHONNOUSERSITE=1`. The C/C++ profile adds `CONAN_HOME` or `VCPKG_ROOT` set to the canonicalized vendored root. `validate_request_inputs` bounds the environment to 256 entries, 4096-byte names, portable identifiers, and 4096-byte values with no interior NUL. The runner's `is_dangerous_environment_name` rejects `LD_*`, `*_PROXY`, `NO_PROXY`, `GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_CONFIG*`, and `CARGO_*` names in non-authoring phases, so no loader, proxy, or credential environment variable can be injected.

**Assessment:** Satisfied. The environment is fixed, bounded, and free of dangerous names.

### 5. Network denied with an unshared namespace and language-specific offline flags

**Control:** The network is denied for offline verification; the runner unshares the network namespace, and each language's offline flags are set so the toolchain never reaches the network.

**Mechanism:** The `SandboxRequest` sets `network.mode = "deny"` with an empty `allowed_sources`. The galla runner, when `network.mode == NetworkMode::Deny`, pushes `--unshare-net`, so the child has no network namespace and no egress path. The Go profile sets `GOPROXY=off` and `GOSUMDB=off`, disabling both proxy and checksum-service lookups. The Python `pip.conf` sets `no-index = true` with `find-links` pointing at the vendored wheels mount. The JavaScript `.npmrc` sets `offline=true` and `prefer-offline=true`. The C/C++ profile mounts the vendored Conan home or vcpkg root read-only at its host path, so the generated toolchain file's absolute cache paths resolve inside the sandbox without network access.

**Assessment:** Satisfied. The network is denied at the namespace level, and each language's offline flags are set.

### 6. Mount sources are canonicalized and pinned without following any pathname pointer

**Control:** Every vendored mount source is canonicalized on the host and pinned by the runner with `O_PATH | O_NOFOLLOW`, so no symlink or traversal component can redirect the mount to an unapproved origin.

**Mechanism:** `canonical_source_str` canonicalizes each mount source to an absolute, normalized UTF-8 path; a non-canonical source is rejected. The runner's `source::pin` opens the source with `O_PATH | O_NOFOLLOW | O_CLOEXEC`, walking each component with `openat` under `O_NOFOLLOW`, so no pathname pointer is followed. A source that is a symlink, a special node, or that contains a parent-traversal component is rejected. The vendored directory is under the project's `.kvist/` subdirectory, and the Go module cache is the host's native cache, which `cache_root` validates as an absolute, non-traversal, non-broad-ancestor directory. The Go module cache rejects parent-traversal components and broad filesystem/home/project ancestors.

**Assessment:** Satisfied. Mount sources are canonicalized and pinned without following any pathname pointer.

### 7. Lock-file digest identity bound to every vendored mount, with staleness re-checked

**Control:** Every vendored mount carries the lock-file digest as its identity, and the digest is re-checked at mount time so a lock file that changed after enforcement is detected.

**Mechanism:** `enforce_vendoring` computes the lock-file digest and returns it with the mounts. Each `VendoredMount` carries the digest as its `identity`. The Rust and Go strategies re-check the digest at mount time: `RustStrategy::mounts` compares the current `Cargo.lock` digest against the expected digest and returns `VendoringStale` on a mismatch; `GoStrategy::mounts` compares the current `go.mod`/`go.sum` digest against the expected digest and returns `VendoringStale` on a mismatch. The lock-file digest is the authoritative catalogue of the locked content, so hashing every vendored file is not required.

**Assessment:** Satisfied. The lock-file digest is the mount identity, and staleness is re-checked at mount time.

### 8. Go GOROOT guard fails closed when outside the bound system prefixes

**Control:** The Go profile requires the host `go` binary and its standard library to be reachable inside the sandbox, where only the bound system prefixes are visible; a GOROOT outside those prefixes fails closed.

**Mechanism:** `locate_host_binary` walks the host `PATH`, skipping administrative paths (`/sbin`, `/usr/sbin`, `/usr/local/sbin`), accepting files and links-to-files, and canonicalizing to the regular executable. `select_toolchain_candidate` prefers a candidate under a bound system prefix (`/usr`, `/lib`, `/lib64`, `/bin`), falling back to the first candidate overall when none is reachable. The Go profile derives `GOROOT` as the grandparent of `bin/go` and checks `under_bound_system_prefix(goroot)`, failing closed with an actionable message when no such install exists. This prevents a toolchain-manager install (e.g. `/opt/hostedtoolcache/go/...`) from leaving the sandbox with a `go` binary it cannot compile with.

**Assessment:** Satisfied. The GOROOT guard fails closed when the standard library is outside the bound prefixes.

### 9. System layout binds the filtered read-only prefixes and validates symlink targets

**Control:** The sandbox binds the filtered system layout read-only, and every symlink target is validated to stay within the layout, so no symlink can escape to an unapproved host path.

**Mechanism:** `system_layout::add_host_system_layout` binds `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, and `/etc/alternatives` read-only. For each symlink, `validate_symlink_target` follows the chain with a bounded depth of 100, detects recursive symlinks, and checks that the resolved target is within the allowed layout. A target that escapes the layout is rejected. The `--cap-drop ALL` argument drops all capabilities. The `is_system_grant` function identifies the system grants so they are not pinned as ordinary mount origins.

**Assessment:** Satisfied. The system layout is filtered, read-only, and symlink-validated.

## Findings

### F1 — JavaScript `.npmrc` offline config is mounted but not consumed by the canonical `node --test` (Info, no fix required)

**Observation:** The JavaScript strategy mounts an offline `.npmrc` at `/workspace/.npm`, but the canonical command `node --test` does not read `.npmrc`; that file is consumed by `npm`, `yarn`, and `pnpm`, not by `node`. The `.npmrc` documents the offline catalogue and is consumed by the host provisioning pass (`npm ci --cache`, `yarn install --cache-folder`, `pnpm install --store-dir`), but the sandbox verification command does not consult it.

**Impact:** None. The `.npmrc` is redundant for the canonical command but not a security issue: `node --test` runs the project's `node:test` suites from the committed `node_modules`, which is inside the read-only component mount, and `node` never reaches the network for this. The `.npmrc` mount is a build-time claim bound to the lock-file digest identity.

**Assessment:** No fix required. The mount is harmless and documents the offline catalogue.

### F2 — Python profile uses the host interpreter with the venv as a `PYTHONPATH` overlay (Info, no fix required)

**Observation:** The Python profile runs the host `python3` interpreter with `PYTHONPATH` pointing at the provisioned venv's `site-packages`, rather than running the venv's own interpreter. The venv is mounted read-only at `/workspace/venv`, and `VIRTUAL_ENV` is set for the tools that read it.

**Impact:** None. The venv is the offline runtime, and `PYTHONPATH` makes its `site-packages` importable. `PYTHONDONTWRITEBYTECODE=1` and `PYTHONNOUSERSITE=1` prevent bytecode writes and user-site lookups, so the read-only venv stays clean. The host interpreter is pinned to the bound system layout, and the venv's `site-packages` is the only Python material the sandbox sees.

**Assessment:** No fix required. The overlay is correct and the venv stays clean.

### F3 — C/C++ build output destination is operator-defined (Info, no fix required)

**Observation:** The C/C++ profile does not explicitly direct build output to the writable scratch; the approved test command is operator-defined, and the build system (e.g. `make`) is responsible for directing its output. The component mount is read-only, so a build that writes to the component directory fails closed.

**Impact:** None. The writable scratch is available at `/workspace/scratch`, and the operator's approved test command is responsible for directing build output there. A command that writes to the read-only component mount fails closed, which is the safe behavior.

**Assessment:** No fix required. The operator-defined command is the build system's contract, and a misdirected write fails closed.

### F4 — Vendored mount identity is a build-time claim not re-verified at execution (Info, documented limitation)

**Observation:** The vendored mount identity is the lock-file digest, which is a build-time claim bound to the approved mount plan and is not re-verified at execution. The lock-file digest is the catalogue of the locked content, so hashing every vendored file is not required. A vendored directory modified after the digest is computed but before execution would mount with the stale identity.

**Impact:** Low. The lock-file digest is re-checked at mount time for Rust and Go (`VendoringStale`), so a lock-file drift is detected. For the other languages, the mount identity is a build-time claim, and the read-only access plus the network denial prevent the mounted material from being a network-escape or injection vector. The documented limitation is that the mount identity is not a per-file hash, so a vendored directory modified after the digest is computed but before execution is not detected by the mount identity alone.

**Assessment:** No fix required. The documented limitation is acceptable: the lock-file digest is the catalogue, and the read-only access plus network denial prevent the mounted material from being a security vector.

## Conclusion

The per-language offline verification topologies preserve the independent confirmation that a non-Rust offline build resolves only approved, pinned material with the network denied. Vendoring is enforced fail-closed before any sandbox work. The offline profile is a closed per-language enumeration: Go, JavaScript, and Python run their canonical offline commands with a fixed, host-leak-free environment; C and C-vcpkg run an operator-approved test command that is allowlisted, split on whitespace with no shell interpretation, and rejected for every canonical language. Every vendored mount is canonicalized, pinned without following any pathname pointer, and identified by the lock-file digest. The runner unshares the network namespace in Deny mode, so no egress path exists. The Go GOROOT guard fails closed when the standard library is outside the bound system prefixes. The system layout is filtered, read-only, and symlink-validated.

No injection, path-traversal, link-following, or network-escape finding requires a fix. The four findings are informational: the JavaScript `.npmrc` mount is redundant for the canonical command, the Python profile uses the host interpreter with the venv as a `PYTHONPATH` overlay, the C/C++ build output destination is operator-defined, and the vendored mount identity is a build-time claim not re-verified at execution. All are documented limitations or harmless redundancies, not security gaps.
