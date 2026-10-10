# Sandbox Environment Analysis

**Last updated:** 2026-10-10 — re-verified against active sandbox instance; Rust toolchain status confirmed working.

## Toolchains

| Language | Compiler/Runtime | Package Manager | Status | Version |
|----------|-----------------|-----------------|--------|---------|
| **C/C++** | `gcc`, `cc`, `g++`, `clang` | None | ✅ Working | GCC 16.2.1, Clang 23.1.1 |
| **Go** | `go` | None (stdlib only) | ✅ Working | go1.27.2-X |
| **Rust** | `rustc`, `cargo`, `rustup`, `rustfmt`, `rustdoc` | Offline Cargo/vendor | ✅ Working | rustup 1.26.0; stable 1.99.0 (default) |
| **Python** | `python3` | `pip`, `pip3` | ✅ Working | 3.14.7 |
| **Node.js** | `node`, `npm`, `npx` | `npm` | ✅ Working | v20.20.2 (npm 12.2.0 warns) |
| **Java** | `java`, `javac`, `jar` | None | ✅ Working | OpenJDK 27 |
| **Ruby** | `ruby`, `gem` | `gem` | ✅ Working | 3.4.10 |
| **Perl** | `perl`, `cpan` | `cpan` | ✅ Working | 5.42.2 |
| **PHP** | `php` | None | ✅ Working | 8.5.11 |
| **Lua** | `lua`, `luac` | None | ✅ Working | 5.5.1 |

## Build & Dev Tools

| Category | Available | Version |
|----------|-----------|---------|
| **Build** | `make`, `cmake`, `ninja` | make 4.4.1, cmake 4.4.4, ninja 1.13.2 |
| **Version Control** | `git` | 2.56.0 |
| **Debug/Inspect** | `gdb`, `valgrind`, `strace`, `ltrace`, `objdump`, `nm`, `readelf` | gdb 18.1, valgrind 3.25.1 |
| **Compression** | `tar`, `gzip`, `gunzip`, `unzip`, `bzip2`, `xz`, `zip` | — |
| **Text Processing** | `sed`, `awk`, `grep`, `cut`, `paste`, `sort`, `uniq`, `wc`, `tr` | — |
| **JSON** | `jq` | 1.8.2 |
| **Network** | `curl`, `wget`, `ssh`, `rsync`, `scp` | No actual network |
| **Containers** | `docker` | Installed, no daemon |
| **Editors** | `vim`, `nano`, `emacs`, `less`, `more` | — |
| **Terminal Multiplexers** | `tmux`, `screen` | 3.7c, 5.0.2 |
| **SQLite CLI** | `sqlite3` | 3.53.4 |

## Rust: Findings

**Status: WORKING — Full toolchain with 8 installed toolchains.**

The Rust ecosystem is fully functional. rustup manages multiple toolchains, with
stable (1.99.0) as the default.

**Installed toolchains:**
- stable-x86_64-unknown-linux-gnu (default) — rustc 1.99.0
- nightly-x86_64-unknown-linux-gnu
- 1.94-x86_64-unknown-linux-gnu
- 1.94.0-x86_64-unknown-linux-gnu
- 1.95.0-x86_64-unknown-linux-gnu
- 1.85.0-x86_64-unknown-linux-gnu
- 1.67.1-x86_64-unknown-linux-gnu
- 1.64.0-x86_64-unknown-linux-gnu

**Installed targets for active toolchain:**
- x86_64-unknown-linux-gnu (host)
- i686-unknown-linux-gnu
- x86_64-apple-darwin
- x86_64-pc-windows-gnu
- x86_64-pc-windows-msvc

**Binary locations:**
- Runtime binaries (on PATH): `/rust/runtime/bin/rustc`, `/rust/runtime/bin/cargo`
- Full toolchain: `/rust/toolchain/`
- rustup home: `/tmp/rustup-home` (after running `/rust/runtime/initialize`)
- Toolchains: `/rust/toolchains/{0-6}` (symlinked via `/tmp/rustup-home/toolchains`)

**Verification:**
```bash
$ rustup --version
rustup 1.26.0 (5af9b9484 2023-04-05)

$ rustup toolchain list
stable-x86_64-unknown-linux-gnu (default)
nightly-x86_64-unknown-linux-gnu
1.94-x86_64-unknown-linux-gnu
...

$ rustc --version
rustc 1.99.0 (b940084d7 2026-09-28)

$ cargo --version
cargo 1.99.0 (5f94df478 2026-08-27)
```

## Missing/Not Available

| Item | Status |
|------|--------|
| **System Package Manager** | ⚠️ `pacman` exists but requires root |
| **Internet Access** | ❌ DNS resolution fails |
| **Docker Daemon** | ❌ Cannot start containers |
| **k8s/TF/Ansible** | ❌ No `kubectl`, `terraform`, `ansible` |
| **autogen** | ❌ No |