# Sandbox Environment Analysis

**Last updated:** 2026-10-13 — re-verified against the active sandbox instance.
Every line below was re-checked live (binary present + version probe). Rust
toolchain status confirmed working.

## Intentional Constraints (by design, not defects)

This is a **hermetic, offline** workspace. The following are deliberate
boundaries, **not** gaps to be worked around:

- **No network access.** DNS does not resolve, there is no `/etc/resolv.conf`
  and not even an `/etc/hosts`. `curl`/`wget` fail on host lookup (`curl`
  exit 6). This is by design.
- **Deliberately absent tooling.** No `kubectl`/`terraform`/`ansible`, no
  `autogen` (the `autoconf`/`automake` family *is* present), no working
  system package manager. Kept out to keep the image small and the boundary
  predictable.
- **Docker client present, daemon absent.** The `docker` binary runs, but
  `/var/run/docker.sock` is missing by design — no containers.

Treat the "Missing/Not Available" table as the **intended** surface area.
Do not attempt to `pacman -S`, `curl`, or `docker run` your way past it.

## Toolchains

| Language | Compiler/Runtime | Package Manager | Status | Version |
|----------|-----------------|-----------------|--------|---------|
| **C/C++** | `gcc`, `cc`, `g++`, `clang` | None | ✅ Working | GCC 16.2.1, Clang 23.1.1 |
| **Go** | `go` | None (stdlib only) | ✅ Working | go1.27.2-X:nodwarf5 linux/amd64 |
| **Rust** | `rustc`, `cargo`, `rustup`, `rustfmt`, `rustdoc` | Offline Cargo/vendor | ✅ Working | rustup 1.26.0; stable 1.99.0 (default) |
| **Python** | `python3` | `pip`, `pip3` | ✅ Working | 3.14.8 (pip 26.2.1) |
| **Node.js** | `node`, `npm`, `npx` | `npm` | ✅ Working | v20.20.2 (npm 12.2.0 warns) |
| **Java** | `java`, `javac`, `jar` | None | ✅ Working | OpenJDK 27 (2026-09-15) |
| **Ruby** | `ruby`, `gem` | `gem` | ✅ Working | 3.4.10 (gem 3.6.9) |
| **Perl** | `perl` | CPAN module (no `cpan` on PATH) | ✅ Working | 5.42.3 (`v5.42.3*`, `5.042003`) |
| **PHP** | `php` | None | ✅ Working | 8.5.11 |
| **Lua** | `lua`, `luac` | None | ✅ Working | 5.5.1 |

Notes:
- **Perl:** the `CPAN` module loads and works, but there is no `cpan`
  command on `PATH`. The core script lives at `/usr/bin/core_perl/cpan`
  (off the default `PATH`). Install Perl deps via the `CPAN` module or treat
  Perl as a stdlib-only tool.
- **npm:** `npm v12.2.0` warns it does not support Node `v20.20.2`
  (it expects `^22.22.2 || ^24.15.0 || >=26.0.0`). It runs, but the pairing
  is intentionally a mismatch to flag the warning path.

## Build & Dev Tools

| Category | Available | Version |
|----------|-----------|---------|
| **Build** | `make`, `cmake`, `ninja` | make 4.4.1, cmake 4.4.4, ninja 1.13.2 |
| **Autotools** | `autoconf`, `autoreconf`, `aclocal`, `automake` | autoconf 2.73, automake 1.19 |
| **Version Control** | `git` | 2.56.0 |
| **Debug/Inspect** | `gdb`, `valgrind`, `strace`, `ltrace`, `objdump`, `nm`, `readelf` | gdb 18.1, valgrind 3.25.1, strace 7.2, binutils 2.47 |
| **Compression** | `tar`, `gzip`, `gunzip`, `unzip`, `bzip2`, `xz`, `zip` | tar 1.35, gzip 1.15, bzip2 1.0.8, xz 5.8.4 |
| **Text Processing** | `sed`, `awk`, `grep`, `cut`, `paste`, `sort`, `uniq`, `wc`, `tr` | GNU sed 4.10, gawk 5.4.1, grep 3.12, coreutils 9.12 |
| **JSON** | `jq` | 1.8.2 |
| **Network** | `curl`, `wget`, `ssh`, `rsync`, `scp` | Installed, **no actual network** |
| **Containers** | `docker` | 29.9.0 installed, no daemon |
| **Editors** | `vim`, `nano`, `emacs`, `less`, `more` | vim 9.2, nano 9.2, emacs 31.1 |
| **Terminal Multiplexers** | `tmux`, `screen` | 3.7c, 5.0.2 |
| **SQLite CLI** | `sqlite3` | 3.54.0 |

## Rust: Toolchain layout & discovery

**Status: WORKING — full toolchain, 8 installed toolchains.**
rustup manages the set; `stable` (1.99.0) is the default.

**Installed toolchains:**
- `stable-x86_64-unknown-linux-gnu` (default) — rustc 1.99.0
- `nightly-x86_64-unknown-linux-gnu`
- `1.94-x86_64-unknown-linux-gnu`
- `1.94.0-x86_64-unknown-linux-gnu`
- `1.95.0-x86_64-unknown-linux-gnu`
- `1.85.0-x86_64-unknown-linux-gnu`
- `1.67.1-x86_64-unknown-linux-gnu`
- `1.64.0-x86_64-unknown-linux-gnu`

**Installed targets for the active (stable) toolchain:**
- `x86_64-unknown-linux-gnu` (host)
- `i686-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-gnu`
- `x86_64-pc-windows-msvc`

**Binary locations:**
- On-PATH runtime: `/rust/runtime/bin/{rustc,cargo,rustfmt,rustdoc,clippy-driver,...}`
- **Default (stable) toolchain:** `/rust/toolchain/` (note: **singular**, no `s`)
- **The other 7 toolchains:** `/rust/toolchains/{0-6}/` (note: **plural**)
- rustup home: `/tmp/rustup-home` (created by `/rust/runtime/initialize`)
- `/tmp/rustup-home/toolchains` → `/rust/rustup-home/toolchains` → the 7 numeric dirs
- `/rust/rustup-home/settings.toml` records `default_toolchain = stable-...`

> **Naming trap:** the default toolchain lives in `/rust/toolchain` (singular)
> while the *others* live in `/rust/toolchains/N` (plural). The two are easy
> to conflate. `rustup` is the source of truth — always use `rustup show
> active-toolchain` / `rustup toolchain list` rather than guessing paths.

### The wrapper scripts are the real interface

`/rust/runtime/bin/rustc` and `/rust/runtime/bin/cargo` are **bash wrappers**,
not raw binaries. They are how the sandbox enforces its rules. Read them before
debugging "why did my build do that". Key behavior:

- **Toolchain selection:** the wrapper calls `rustup show active-toolchain`
  and execs `rustup run <selected> rustc/cargo ...`. You can override with
  the `+toolchain` syntax, e.g. `cargo +1.67.1 build`, `rustc +nightly x.rs`.
- **`cargo` always injects:** `--offline --locked` **and**
  `--config 'source.crates-io.replace-with="vendored-sources"'
  --config 'source.vendored-sources.directory="/rust/vendor"'`.
  So `cargo build` here is *always* offline, lockfile-pinned, and vendored
  against `/rust/vendor` — even if your `.cargo/config.toml` disagrees.
- **`KVIST_CARGO_WORKDIR`:** if set, the `cargo` wrapper `cd`s into it first.
- **`KVIST_COMPONENT_BUILD_DIR`:** the `rustc` wrapper rewrites any `src/*`
  or `tests/*` argument rooted in that dir to `/workspace/component/...`
  (and `cargo` honors it for workdir). This is the Kvist component-build
  remapping; it only fires when that env var is set.

Relevant environment (set for every shell):

```
HOME=/tmp
PATH=/rust/runtime/bin:/rust/toolchain/bin:/rust/user-cargo-bin:/usr/bin:/bin:/usr/sbin:/sbin
RUSTUP_HOME=/tmp/rustup-home
RUSTUP_AUTO_INSTALL=0
CARGO_HOME=/tmp/cargo-home
CARGO_NET_OFFLINE=true
CARGO_TARGET_DIR=/tmp/target
RUSTC=/rust/runtime/bin/rustc
RUSTDOC=/rust/runtime/bin/rustdoc
```

> **`RUSTUP_AUTO_INSTALL=0`** means rustup will never silently download a
> missing toolchain — you get a hard error. This is intentional and
> consistent with the offline boundary.

## Rust: Where builds end up

**Live builds write to `/tmp/target`, not `/workspace/target`.**

- `CARGO_TARGET_DIR=/tmp/target` is set in the environment, so a fresh
  `cargo build`/`cargo test` in the sandbox lays down artifacts under
  `/tmp/target/{debug,release,deps,build,...}`. `/tmp/target` does not
  pre-exist; the build creates it.
- `/workspace/target/` **does** exist and contains a `release/` tree with
  host-built binaries (`kvist`, `galla-runner`, `blad`, `libblad.rlib`, ...).
  These are **stale host leftovers**, not the output of an in-sandbox build.
  Do not assume `/workspace/target/release/*` reflects the code you just
  compiled. If you need the real artifact, point at `/tmp/target`.

Consequence: anything that reads "the build output" must be told which target
dir is authoritative (`/tmp/target` for live work) or it will silently consume
the old `/workspace/target` tree.

## Rust: Where vendoring lives

There are **three** vendored-source locations, and only one is authoritative in
the sandbox. The metadata files reference two host paths that **do not exist
here** — a major discovery trap.

| Location | What it is | Authoritative in sandbox? |
|----------|-----------|---------------------------|
| `/rust/vendor/` | Read-only global vendored snapshot, ~300 crates. This is what the `cargo` wrapper's injected `--config ...directory="/rust/vendor"` actually uses. | ✅ **Yes — the only one that works.** |
| `/workspace/.kvist/vendored/` | Project-local copy produced by `kvist vendor` (`just vendor` → `cargo run --locked -p kvist -- vendor .`). | Project-scoped; not the sandbox build source. |
| `/workspace/vendored` | Referenced by `.kvist/sandbox-cargo/config.toml` (`source.vendored-sources.directory="/workspace/vendored"`) and by `vendoring-v1.json` as `sandbox_vendored_mount`. | ❌ **Does not exist.** Stale. |

**Stale metadata (do not trust the paths inside):**
- `/workspace/.kvist/vendoring-v1.json` records
  `project_root: /opt/proj/kvist`, `vendored_dir: /opt/proj/kvist/.kvist/vendored`,
  `sandbox_cargo_dir: /opt/proj/kvist/.kvist/sandbox-cargo`,
  `sandbox_vendored_mount: /workspace/vendored` — all host paths that are
  absent in the sandbox. Only its `lockfile_digest` and `verified` fields are
  meaningful.
- `/workspace/.kvist/sandbox-cargo/config.toml` is a `kvist vendor`-managed
  config (header: "Do not edit by hand") that points at the missing
  `/workspace/vendored`. It is **not** what the build actually uses, because
  the wrapper's command-line `--config` overrides it with `/rust/vendor`.
- `/workspace/.kvist/rust-toolchain.json` pins
  `toolchain_root: /home/stefan/.rustup/toolchains/stable-...` and a matching
  `cargo_path`/`cargo_digest` — **`/home/stefan` does not exist here.** The
  only trustworthy field is `channel: "default"`. The host-absolute paths are
  stale artifacts of the provisioning machine.

**Rule of thumb:** in the sandbox, the vendor source of truth is `/rust/vendor`
(wired in by the wrapper). Project-local vendoring and the `.kvist` manifests
are for the host/CI provisioning flow, not for live sandbox builds.

## Missing / Not Available (intentional)

| Item | Status |
|------|--------|
| **Internet access** | ❌ DNS fails, no `/etc/hosts` or `/etc/resolv.conf` — by design |
| **System package manager** | ⚠️ `pacman` 7.1.0 is installed and its CLI runs, but there is no functioning package DB and no root — effectively non-functional |
| **Docker daemon** | ❌ Client present, no daemon — no containers |
| **k8s / Terraform / Ansible** | ❌ No `kubectl`, `terraform`, `ansible` |
| **autogen** | ❌ Not installed (`autoconf`/`automake` *are* present) |
| **Perl `cpan` command** | ⚠️ Not on `PATH` (core script at `/usr/bin/core_perl/cpan`); use the `CPAN` module |

Every row here is a **deliberate** boundary. Do not treat them as bugs.

## Recommendations: improving toolchain & artifact discoverability

These are the concrete frictions found during this review, ordered by impact.

1. **Make the "live" target dir explicit and unambiguous.**
   `/workspace/target/release` (host leftovers) vs `/tmp/target` (live builds)
   is the single most likely source of a wrong answer. Options, in order of
   preference:
   - Have the `cargo` wrapper `echo` the effective `CARGO_TARGET_DIR` on a
     `--verbose`/`--dry-run` path, or write a one-line sentinel into
     `/tmp/target` (e.g. a `.target_root` marker) so tooling can distinguish
     "live" from "stale host tree".
   - Or remove `/workspace/target` from the sandbox image entirely so the only
     target dir that exists is the real one.
   - At minimum, document (here) that `/workspace/target` is stale — this file
     now does.

2. **Coalesce or clearly label the three vendor locations.**
   Two of the three (`/workspace/vendored`, `/workspace/.kvist/vendored`) are
   not what the build uses. Either:
   - mount the authoritative snapshot at the path the manifests already name
     (`/workspace/vendored`), so `.kvist/vendoring-v1.json` and
     `.kvist/sandbox-cargo/config.toml` become true; or
   - add a one-line "SANDBOX SOURCE OF TRUTH" header to
     `/rust/vendor/.cargo-ok`-style marker and a `README.md` at `/rust/vendor`
     stating it is the canonical offline source.

3. **Sanitize host-absolute paths out of the checked-in manifests.**
   `.kvist/rust-toolchain.json` (`/home/stefan/...`) and
   `.kvist/vendoring-v1.json` (`/opt/proj/kvist/...`, `/workspace/vendored`)
   carry paths that do not exist in the sandbox. They actively mislead anyone
   reading them. Provisioning should rewrite them to sandbox-relative or
   clearly mark them `host_only: true`.

4. **Provide a single "what am I running?" discovery command.**
   Today you must know to run `rustup show active-toolchain`,
   `cat /rust/runtime/bin/cargo`, and inspect env vars separately. A tiny
   helper (e.g. `kvist-env` / `sandbox-doctor`) that prints
   {active toolchain, effective CARGO_TARGET_DIR, vendor dir in force,
   offline=yes/no network, list of the 8 toolchains} would collapse the
   multi-step hunt into one call. The wrapper scripts already have all this
   information; expose it.

5. **Break the `/rust/toolchain` vs `/rust/toolchains` naming trap.**
   The default toolchain is singular, the rest plural. A symlink or alias
   (`/rust/toolchains/default -> /rust/toolchain`) plus a short note would
   prevent path-guessing errors.

6. **Put `cpan` (or a clear signpost) on PATH / in this doc.**
   Perl's `CPAN` module works but the `cpan` launcher is off-PATH. Either
   symlink `/usr/bin/cpan -> /usr/bin/core_perl/cpan` or keep the note here so
   nobody assumes Perl has no package tooling.

7. **Keep the intentional-constraints section up top and explicit.**
   The no-network and missing-tool facts are *by design*; stating that up
   front (as this file now does) prevents an agent from wasting time trying to
   `curl`, `pacman -S`, or `docker run` against a wall.
