# Sandbox Environment Analysis

This document describes the toolchain availability and environment behavior
that can be expected in the Galla sandbox namespace. It distinguishes between
tools that are part of the core sandbox specification and tools whose
availability depends on the specific host system.

For AI agents operating in this sandbox, see also [`AGENTS.md`](AGENTS.md)
for a concise agent-focused environment guide.

For authoritative topology see
[`docs/sandbox-toolchains-and-vendoring.md`](docs/sandbox-toolchains-and-vendoring.md).

## Core Toolchain (expected on any host)

The following language toolchains are expected to be available on any properly
configured Galla sandbox instance:

| Language | Compiler/Runtime | Package Manager | Build Tool |
|----------|-----------------|-----------------|------------|
| **C/C++** | `gcc`, `cc`, `g++`, `clang` | — | `make` |
| **Go** | `go` | — (stdlib only) | — |
| **Rust** | `rustc`, `cargo`, `rustup` | Offline Cargo/vendor | `cargo` |
| **Python** | `python3` | `pip`, `pip3` | — |
| **Node.js** | `node`, `npm`, `npx` | `npm` | — |

Additional build system tools expected:

| Category | Tools |
|----------|-------|
| **Build** | `make`, `cmake`, `ninja` |
| **Autotools** | `autoconf`, `autoreconf`, `aclocal`, `automake` |
| **Version Control** | `git` |

**Note on versions:** Specific versions of these toolchains will vary by host
and by when the sandbox environment was provisioned. Do not hardcode version
assumptions in build scripts or tests. Use feature detection or version
querying (e.g., `rustc --version`, `python3 --version`) when version-specific
behavior matters.

## Host-Dependent Tools (availability may differ)

The following tools may or may not be available depending on the host system's
package installation. Treat these as optional or use version detection before
relying on them.

### Additional Language Runtimes

| Language | Commands | Typical Use |
|----------|----------|-------------|
| **Java** | `java`, `javac`, `jar` | JVM applications |
| **Ruby** | `ruby`, `gem` | Ruby scripts and gems |
| **Perl** | `perl` | Perl scripts (CPAN module available, `cpan` command often off-PATH) |
| **PHP** | `php` | PHP scripts |
| **Lua** | `lua`, `luac` | Lua scripts |

### Development Utilities

| Category | Tools | Notes |
|----------|-------|-------|
| **Debug/Inspect** | `gdb`, `valgrind`, `strace`, `ltrace`, `objdump`, `nm`, `readelf` | Binary debugging and inspection |
| **Compression** | `tar`, `gzip`, `gunzip`, `unzip`, `bzip2`, `xz`, `zip` | Archive operations |
| **Text Processing** | `sed`, `awk`, `grep`, `cut`, `paste`, `sort`, `uniq`, `wc`, `tr` | GNU core text tools |
| **JSON** | `jq` | JSON querying and manipulation |
| **Editors** | `vim`, `nano`, `emacs`, `less`, `more` | Text editing and viewing |
| **Terminal Multiplexers** | `tmux`, `screen` | Session management |
| **SQLite** | `sqlite3` | SQLite database CLI |
| **Network** | `curl`, `wget`, `ssh`, `rsync`, `scp` | Installed but **no actual network** |
| **Containers** | `docker` | Client often installed, no daemon socket |

### Tools That May Be Missing

Some tools that are available on some hosts may be absent on others:
- `kubectl`, `terraform`, `ansible` — infrastructure tools
- `autogen` — some autotools workflows
- `pacman` or other package managers — no host provisioning authority

## Intentional Constraints (by design)

These are deliberate sandbox boundaries, not defects:

- **No network access.** DNS does not resolve, there is no `/etc/resolv.conf`
  or `/etc/hosts`. `curl`/`wget` fail on host lookup.
- **No host provisioning authority.** A sandbox agent cannot install host
  packages.
- **No host Docker daemon.** The `docker` client may be installed, but
  `/var/run/docker.sock` is absent by design — no host containers.

Do not attempt to `pacman -S`, `curl`, or `docker run` your way past the
boundary. Installed incidental tools and missing tools vary by host.

- **Forbidden-command denylist.** The sandbox shell tool applies an advisory
  command denylist. `git commit` is currently on this denylist, while `git
  add`, `git status`, and `git diff` are permitted. Commands on the denylist
  return a policy violation rather than executing. This restriction is global
  to the sandbox, not per-directory or per-component.

## Rust Toolchain Layout & Discovery

The Rust toolchain uses a layered discovery mechanism. The following describes
the layout and how to work with it.

### Toolchain Selection

The wrapper scripts invoke `rustup` to determine which toolchain to use.
Selection can be overridden with the `+toolchain` syntax:

```bash
cargo +1.67.1 build
rustc +nightly x.rs
```

Query the current configuration rather than assuming specific paths:

```bash
rustup show active-toolchain
rustup toolchain list
```

### Vendor Snapshot

Prepared authoring uses `/rust/vendor` for the offline vendor snapshot. The
`cargo` wrapper injects vendor config pointing to this location.

### Build Artifacts

- **Skott workspace authoring:** `/workspace/target` (persistent, writable)
- **Maerg protected Rust authoring:** `/tmp/target` (invocation-local scratch)
- **Galla native inventory fixture:** `/tmp/target` (invocation-local scratch)
- **Maerg closed Cargo verification:** Request-defined target scratch under
  `/workspace`
- **Host/CI build of Kvist and runner:** Cargo-configured host target directory

### `/tmp` is Ephemeral

`/tmp` is a per-invocation ephemeral `tmpfs`. Anything written there
(including `/tmp/rustup-home` or `/tmp/target`) is discarded between shell
invocations. Do not rely on `/tmp` for persistent state. Use `/workspace` for
artifacts that must survive across invocations.

### Naming Trap

The default toolchain lives in `/rust/toolchain` (singular) while the *others*
live in `/rust/toolchains/N` (plural). These are easy to conflate. Always
use `rustup show active-toolchain` / `rustup toolchain list` rather than
guessing paths.

## Observed on This Host (Reference Only)

The following version observations are from the host where this document was
last verified. They are provided as reference, not as specifications for
other hosts.

### Toolchain Versions Observed

| Tool | Version |
|------|---------|
| GCC | 16.2.1 |
| Clang | 23.1.1 |
| Python | 3.14.8 |
| pip | 26.2.1 |
| Node | v20.20.2 |
| npm | 12.2.0 (warns about Node version compatibility) |
| Go | go1.27.2-X:nodwarf5 linux/amd64 |
| Rust stable | 1.99.0 (default) |
| Rust nightly | Available |
| Rust other versions | 1.95.0, 1.94.0, 1.94, 1.85.0, 1.67.1, 1.64.0 |
| make | 4.4.1 |
| cmake | 4.4.4 |
| ninja | 1.13.2 |
| git | 2.56.0 |
| jq | 1.8.2 |
| tmux | 3.8 |
| screen | 5.0.2 |

### Additional Tools Observed

| Tool | Version |
|------|---------|
| Java | OpenJDK 27 |
| Ruby | 3.4.10 |
| Perl | 5.42.3 |
| PHP | 8.5.11 |
| Lua | 5.5.1 |
| gdb | 18.1 |
| valgrind | 3.25.1 |
| strace | 7.2 |
| autoconf | 2.73 |
| automake | 1.19 |
| sed | 4.10 |
| awk | 5.4.1 |
| grep | 3.12 |
| coreutils | 9.12 |
| tar | 1.35 |
| gzip | 1.15 |
| bzip2 | 1.0.8 |
| xz | 5.8.4 |
| docker (client) | 29.9.0 (no daemon) |
| vim | 9.2 |
| nano | 9.2 |
| emacs | 31.1 |
| sqlite3 | 3.54.0 |

### Observed Quirks on This Host

- **npm compatibility warning:** `npm v12.2.0` warns it does not support
  Node `v20.20.2` (it expects `^22.22.2 || ^24.15.0 || >=26.0.0`). It runs,
  but this pairing is a known mismatch.
- **cpan off-PATH:** The Perl CPAN module is available, but the `cpan`
  launcher command lives at `/usr/bin/core_perl/cpan` (off the default PATH).
- **Duplicate rustup binary:** `/usr/bin/rustup` (1.29.1, System) is used by
  wrapper scripts, while `/rust/user-cargo-bin/rustup` (1.26.0, host shim) is
  on PATH. The wrapper's `/usr/bin/rustup` is authoritative.

## Quick Environment Probe

Run this snippet to determine your sandbox environment:

```bash
echo "=== Sandbox Environment Probe ==="
echo "Rust: $(rustc --version 2>/dev/null)"
echo "Cargo: $(cargo --version 2>/dev/null)"
echo "Python: $(python3 --version 2>/dev/null)"
echo "Node: $(node --version 2>/dev/null)"
echo "Go: $(go version 2>/dev/null)"
echo "GCC: $(gcc --version 2>/dev/null | head -1)"
echo "Clang: $(clang --version 2>/dev/null | head -1)"
echo "Make: $(make --version 2>/dev/null | head -1)"
echo "CMake: $(cmake --version 2>/dev/null | head -1)"
echo "Git: $(git --version 2>/dev/null)"
echo "Active Rust toolchain: $(rustup show active-toolchain 2>/dev/null)"
echo "=== Available Rust toolchains ==="
rustup toolchain list 2>/dev/null
echo "=== PATH ==="
echo "$PATH"
echo "=== CARGO_TARGET_DIR ==="
echo "$CARGO_TARGET_DIR"
echo "=== KVIST_CARGO_WORKDIR ==="
echo "$KVIST_CARGO_WORKDIR"
echo "=== KVIST_COMPONENT_BUILD_DIR ==="
echo "$KVIST_COMPONENT_BUILD_DIR"
```