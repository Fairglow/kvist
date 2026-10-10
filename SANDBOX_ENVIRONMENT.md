# Sandbox Environment Analysis (Updated)

**Last updated:** 2026-10-10 — native Rust sandbox chain repaired and re-verified.
The inventory below originated in a generic sandbox session; it is not a host
inventory or a guarantee that every execution phase grants the same resources.

## Toolchains

| Language | Compiler/Runtime | Package Manager | Status | Version |
|----------|-----------------|-----------------|--------|---------|
| **C/C++** | `gcc`, `cc`, `g++`, `clang` | None | ✅ Working | GCC 16.2.1, Clang 23.1.1 |
| **Go** | `go` | None (stdlib only) | ✅ Working | go1.27.2-X |
| **Rust** | `rustc`, `cargo`, `rustdoc`, `rustfmt`, Clippy | Offline Cargo/vendor snapshot | ✅ Working in Skott's prepared workspace sandbox and maerg's offline verification | Stable 1.99.0; nightly and exact 1.95.0 selection also tested |
| **Python** | `python3` | `pip`, `pip3` | ✅ Working | 3.14.7 |
| **Node.js** | `node`, `npm`, `npx` | `npm` | ✅ Working | v20.20.2 |
| **Java** | `java`, `javac`, `jar` | None | ✅ Working | OpenJDK 27 |
| **Ruby** | `ruby`, `gem` | `gem` | ✅ Working | No registry |
| **Perl** | `perl`, `cpan` | `cpan` | ✅ Working | No CPAN |
| **PHP** | `php` | None | ✅ Working | CLI only |
| **Lua** | `lua`, `luac` | None | ✅ Working | LuaJIT 2.1 |

## Build & Dev Tools

| Category | Available |
|----------|-----------|
| **Build** | `make`, `cmake` (3.33.4), `ninja` (1.12.1), `autogen` |
| **Version Control** | `git` (2.56.0), `mercurial` |
| **Debug/Inspect** | `gdb`, `valgrind`, `strace`, `ltrace`, `objdump`, `nm`, `readelf` |
| **Compression** | `tar`, `gzip`, `gunzip`, `unzip`, `bzip2`, `xz`, `zip` |
| **Text Processing** | `sed`, `awk`, `grep`, `cut`, `paste`, `sort`, `uniq`, `wc`, `tr` |
| **JSON** | `jq` |
| **Network** | `curl`, `wget`, `ssh`, `rsync`, `scp` (no actual network) |
| **Containers** | `docker` (installed, no daemon) |
| **Editors** | `vim`, `nano`, `emacs`, `less`, `more` |
| **Terminal Multiplexers** | `tmux` (3.7c), `screen` (5.0.2) |
| **SQLite CLI** | `sqlite3` (3.53.4) |

## Rust: findings and repaired chain

The original session saw only system rustup proxies, not a concrete compiler.
That remains possible in a generic system-only sandbox. It is not evidence that
the host has no toolchains or that the prepared Rust execution path is unusable.

Native reproduction found four wiring defects:

1. Generated rustup registrations linked to inaccessible host paths and
   advertised installations that were not mounted.
2. `RUSTUP_TOOLCHAIN=stable` ignored an explicitly selected alternate pin.
3. `HOME=/workspace/home` named a nonexistent directory.
4. Maerg generic authoring generated a temporary rustup home that was dropped
   before Bubblewrap mounted it; it also changed the approved environment and
   added a grant after calculating the mount-plan identity.

Skott now automatically registers every validated installed version and target.
The initial selection uses `/rust/toolchain`; other versions use read-only
`/rust/toolchains/N` mounts, with sandbox-native links and generated read-only
rustup settings. The project pin remains the initial selection; the agent can
choose another version using `cargo +<name>` or `rustup run`. Compiler/rustdoc
wrappers follow that selection. Automatic installation remains disabled. Normal PATH
Cargo resolves through `/rust/runtime/bin/cargo`, enforcing offline/locked
defaults and the immutable `/rust/vendor` snapshot. `HOME=/tmp`, Cargo home and
build output are private invocation scratch. Host credentials/settings remain
unmounted. Compiler, standard libraries, requested formatter/Clippy companions
and generated settings are validated; tracked drift fails before dispatch.

Maerg generic authoring no longer creates rustup state or replaces approved
environment entries. It remains a protected effect boundary, **not** an
interactive Rust build environment. Maerg Rust verification uses its existing
separate, vendored, network-denied Cargo topology. This repair does not widen
maerg task authority or approve any component.

### Native verification

`skott/tests/rust_build_environment.rs` exercises actual Bubblewrap execution:
PATH and rustup discovery; selected stable/nightly pins; an exact-version
minimal installation; compilation/linking/binary execution; unit, integration
and documentation tests; formatting, Clippy and documentation generation;
vendored dependency resolution despite forged host paths and workspace
mutation; missing/stale lock refusal; pin drift; failure propagation; private
scratch, read-only resources, credential exclusion and network denial.
The repository's **own galla library tests** also run inside the Skott workspace
sandbox, using the existing 360 MiB vendor tree. Preparation remains bounded to
30 seconds; an initial full-tree trial hit that bound, while subsequent measured
trials completed preparation in about six seconds. The limit is not weakened:
slow/oversized preparations fail explicitly before execution.

`maerg/tests/sandbox_toolchain.rs` checks generic authoring with unchanged
approved environment and no rustup state. All five native trials pass; generic
verification explicitly grants concrete executables, and the build trial
compiles its actual isolated, separately locked fixture rather than the
repository root. These tests do not authorize ambient rustup proxies. The existing
`offline_cargo_verification_e2e` exercises host provisioning, backend/runner
identity probing, enforced vendoring and real locked Cargo tests inside the
network-denied verification sandbox. Optional Cargo-installed tools are tested
only with host provisioning; this is not a promise to install them.

### Diagnostics and repeatable checks

Enable `SKOTT_LOG=skott::toolchain::rust_environment=debug,skott::sandbox=trace`
(or `RUST_LOG` as fallback) for preparation stages/timings, identities,
mount metadata, environment **names**, dispatch bounds and completion flags.
Command text, environment values and captured output are not tracing payloads.
See [sandbox toolchains and vendoring](docs/sandbox-toolchains-and-vendoring.md)
for exact native commands and the distinct maerg/Skott boundaries.

## Missing/Not Available

| Item | Status |
|------|--------|
| **Rust provisioning inside sandbox** | Intentionally unavailable; provision requested toolchains/components on the host |
| **System Package Manager** | ❌ `pacman` exists but requires root to use |
| **Internet Access** | ❌ DNS resolution fails |
| **Docker Daemon** | ❌ Cannot start containers |
| **k8s/TF/Ansible** | ❌ No `kubectl`, `terraform`, `ansible` |
| **Bash completion** | ❌ Not loaded |

## Key Constraints

- **Filesystem**: Read-only system, write to `/workspace` only
- **Memory**: ~15.7GB (tmpfs root)
- **Disk**: 686GB free on /workspace
- **User**: UID 1000, no username
- **Network**: Isolated (DNS fails, curl returns 000)
- **Home directory**: `/tmp` (not persistent)

## Recommendations for Better Usability

### Critical
1. Provision the selected Rust channel and requested components on the host;
   use the prepared Rust workspace/verification path, not a generic proxy-only
   sandbox. Provision matching locks and vendors separately.

### Nice to Have
2. **Environment variable for workspace**: `export WORKSPACE=/workspace` to avoid hardcoded paths
3. **Enable bash completion**: Speeds up CLI usage
4. **Add `fd` and `ripgrep`**: Modern alternatives to `find`/`grep`

Rust no longer requires switching languages or attempting a network installation
inside the sandbox. CI runs the self-contained native Rust chain explicitly;
repository-vendor and Cargo-installed-tool trials require separately provisioned
host material and are documented opt-in checks.

## Verification limits outside this repair

The wider maerg dogfood suite retains an unchanged bare-Git fixture failure
(`accepted_commit::component_accept_commit_preserves_unrelated_state_and_commits_only_reported_paths`):
its expected remote `refs/heads/main` is absent. Read-only project inspection
also reports existing invalid Blad artifacts and six pre-existing unsorted
requirement sets in Skott's queue, confirmed unchanged against HEAD. Changed
maerg intent/queue/record and Skott intent/record validate structurally.
These unrelated failures were not silently repaired; this report does not
claim whole-repository acceptance or unconditional sandbox certification.
