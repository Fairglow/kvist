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
    cargo build --locked --workspace --all-features
    install -D -m 755 target/debug/kvist ~/.kvist/bin/kvist
    install -D -m 755 target/debug/galla-runner ~/.kvist/bin/galla-runner
    install -D -m 755 target/debug/sav-run ~/.kvist/bin/sav-run
    install -D -m 755 target/debug/skott ~/.kvist/bin/skott
    install -D -m 755 target/debug/skott-file-tool ~/.kvist/bin/skott-file-tool

fix:
    cargo clippy --fix --locked --workspace --all-targets --all-features

format:
    cargo fmt --check

lint:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

msrv:
    cargo +1.95.0 test --locked --workspace --all-features

reformat:
    cargo fmt

release:
    cargo build --locked --workspace --release --all-features
    install -D -m 755 target/release/kvist ~/.kvist/bin/kvist
    install -D -m 755 target/release/galla-runner ~/.kvist/bin/galla-runner
    install -D -m 755 target/release/sav-run ~/.kvist/bin/sav-run
    install -D -m 755 target/release/skott ~/.kvist/bin/skott
    install -D -m 755 target/release/skott-file-tool ~/.kvist/bin/skott-file-tool

# Install release binaries to system-wide location (/usr/local/bin)
install:
    just release
    sudo install -D -m 755 target/release/kvist /usr/local/bin/kvist
    sudo install -D -m 755 target/release/galla-runner /usr/local/bin/galla-runner
    sudo install -D -m 755 target/release/sav-run /usr/local/bin/sav-run
    sudo install -D -m 755 target/release/skott /usr/local/bin/skott
    sudo install -D -m 755 target/release/skott-file-tool /usr/local/bin/skott-file-tool

test:
    kvist vendor
    cargo nextest run --locked --workspace --all-features --test-threads num-cpus

audit:
    cargo deny --all-features --locked check advisories bans licenses sources
