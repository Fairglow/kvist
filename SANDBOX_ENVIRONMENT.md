# Sandbox Environment Analysis

## Toolchains

| Language | Compiler/Runtime | Package Manager | Notes |
|----------|-----------------|-----------------|-------|
| **C/C++** | `gcc`, `cc`, `g++`, `c++`, `clang`, `clang++` | None | GCC 16.2.1, Clang 21.1.4 |
| **Go** | `go`, `gofmt`, `godoc` | None (stdlib only) | go1.27.2-X |
| **Rust** | `rustc`, `cargo` | None | No crates.io access |
| **Python** | `python3`, `python` | `pip`, `pip3` | 3.14.7, no apt-get |
| **Node.js** | `node`, `npm`, `npx` | `npm` | v20.20.2, no registry |
| **Java** | `java`, `javac`, `jar` | None | OpenJDK 21 |
| **Ruby** | `ruby`, `gem` | `gem` | No rubygems.org |
| **Perl** | `perl`, `cpan` | `cpan` | No CPAN |
| **PHP** | `php` | None | CLI only |
| **Lua** | `lua`, `luac` | None | LuaJIT 2.1 |

## Build & Dev Tools

| Category | Available |
|----------|-----------|
| **Build** | `make`, `cmake` (3.33.4), `ninja` (1.12.1), `autogen` |
| **Version Control** | `git` (2.56.0), `mercurial` |
| **Debug/Inspect** | `gdb`, `valgrind`, `strace`, `ltrace`, `objdump`, `nm`, `readelf` |
| **Compression** | `tar`, `gzip`, `gunzip`, `unzip`, `bzip2`, `xz` |
| **Text Processing** | `sed`, `awk`, `grep`, `cut`, `paste`, `sort`, `uniq`, `wc`, `tr` |
| **JSON** | `jq` |
| **Network** | `curl`, `wget`, `ssh`, `rsync`, `scp` (no actual network) |
| **Containers** | `docker` (installed, no daemon) |
| **Editors** | `vim`, `nano`, `emacs`, `less`, `more` |

## Missing/Not Available

| Item | Status |
|------|--------|
| **System Package Manager** | ❌ No `apt-get`, `yum`, `dnf`, `apk` |
| **zip** | ❌ Only `unzip` available |
| **Internet Access** | ❌ DNS resolution fails |
| **Docker Daemon** | ❌ Cannot start containers |
| **k8s/TF/Ansible** | ❌ No `kubectl`, `terraform`, `ansible` |
| **tmux/screen** | ❌ No terminal multiplexers |
| **SQLite CLI** | ❌ Library only |
| **Bash completion** | ❌ Not loaded |

## Key Constraints

- **Filesystem**: Read-only system, write to `/workspace` only
- **Memory**: ~15.7GB (tmpfs root)
- **Disk**: 686GB free on /workspace
- **User**: UID 1000, no username
- **Network**: Isolated (expected for sandbox)
- **Home directory**: `/tmp` (not persistent)

## Recommendations for Better Usability

1. **Environment variable for workspace**: `export WORKSPACE=/workspace` to avoid hardcoded paths
2. **Persistent home**: Mount `/workspace` as home directory for user-specific config
3. **Add zip**: Useful for creating archives
4. **Add tmux/screen**: Helpful for long-running commands
5. **Enable bash completion**: Speeds up CLI usage
6. **Consider adding**: `sqlite3` CLI, `fd` (better find), `ripgrep` (better grep)

---

## Rust Toolchain & rustup Investigation

**Findings:**

The Rust toolchain is installed system-wide at `/usr/sbin/rustc` and `/usr/sbin/cargo`, working directly without any mediation. However, `rustup` (v1.29.1, also at `/usr/sbin/rustup`) is **completely unaware** of this installation and reports:

- "no installed toolchains"
- "no active toolchain"
- "no `rustc` is currently active"

**What was tested:**

1. **`rustup default stable`** — Failed because it tries to download from `https://static.rust-lang.org/dist/` but DNS resolution fails in the sandbox (no network).

2. **`rustup toolchain link system /usr/sbin/rustc`** — Failed with "not a directory: `/usr/sbin/rustc/lib`" because `link` expects a toolchain *directory* with a `lib/` subdirectory, not the binary directly.

3. **`rustup toolchain list`** — Shows empty list.

4. **`rustup show`** — Reports default host as `x86_64-unknown-linux-gnu` with rustup home at `/tmp/.rustup`, but no toolchain installed.

**Root cause:**

The sandbox image provides the Rust compiler directly (via the system package manager used to build the image), while also including `rustup` as an installed binary. However, rustup expects to manage toolchains in its own directory structure (`~/.rustup/toolchains/`), which doesn't exist here. This is a **misconfigured setup** where the two Rust management layers are disconnected.

**Workarounds:**

- **Option 1 (recommended)**: Bypass rustup entirely and use `rustc`/`cargo` directly. They work fine without rustup's mediation.
- **Option 2**: Set `RUSTUP_HOME` and `PATH` to point rustup to a rustup-managed directory structure (requires manually creating the layout).
- **Option 3**: Create a proper rustup-managed toolchain directory with symlinks to the system binaries.

**For sandbox work, Option 1 is the most practical.** The sandbox provides the Rust toolchain directly, not through rustup's management layer.
