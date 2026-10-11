# Optional convenience recipes using only Cargo and Rustup.
#
# `just` itself is optional; the equivalent Cargo commands are documented in
# README.md. The default recipes do not depend on cargo-nextest, cargo-outdated,
# unbuffer, nightly Rust, or generated log files.

all: audit format lint build test

# `kvist vendor`: reconcile the vendored registry against Cargo.lock. When the
# registry is stale and a sandbox is configured, a network-allow acquisition
# sandbox (cargo fetch) provisions it before the effect sandbox executes; the
# fetched material is repacked into .kvist/vendored offline. A no-op when the
# registry already satisfies the lock.
vendor:
    cargo run --locked -p kvist -- vendor .

build:
    cargo build --locked --workspace --all-features \
                --message-format=json-diagnostic-rendered-ansi > .kvist/cargo-build.log
    python3 tools/cargo_install_binaries.py .kvist/cargo-build.log

fix:
    cargo clippy --fix --locked --workspace --all-targets --all-features

format:
    cargo fmt --check

lint:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

msrv:
    cargo +1.95.0 test --locked --workspace --all-features

old:
    cargo outdated --depth=1

reformat:
    cargo fmt

release:
    cargo build --locked --workspace --release --all-features \
                --message-format=json-diagnostic-rendered-ansi > .kvist/cargo-release.log
    python3 tools/cargo_install_binaries.py .kvist/cargo-release.log

# Install release binaries to system-wide location (/usr/local/bin)
# Uses cargo metadata to detect the actual target directory (supports pooled targets)
install: release
    TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | python3 -c "import sys, json; print(json.load(sys.stdin)['target_directory'])") && \
    sudo install -D -m 755 "${TARGET_DIR}/release/kvist" /usr/local/bin/kvist && \
    sudo install -D -m 755 "${TARGET_DIR}/release/galla-runner" /usr/local/bin/galla-runner && \
    sudo install -D -m 755 "${TARGET_DIR}/release/sav-run" /usr/local/bin/sav-run && \
    sudo install -D -m 755 "${TARGET_DIR}/release/skott" /usr/local/bin/skott && \
    sudo install -D -m 755 "${TARGET_DIR}/release/skott-file-tool" /usr/local/bin/skott-file-tool

# Print the target directory on the host
target:
    cargo metadata --format-version 1 --no-deps | python3 -c "import sys, json; print(json.load(sys.stdin)['target_directory'])"

test:
    kvist vendor
    cargo nextest run --locked --workspace --all-features --test-threads num-cpus

audit:
    cargo deny --all-features --locked check advisories bans licenses sources
