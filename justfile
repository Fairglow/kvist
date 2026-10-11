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

# Install debug binaries to user-specific Kvist location (~/.kvist/bin)
install:
    cargo build --locked --workspace --all-features
    mkdir -p ~/.kvist/bin
    cp target/debug/kvist ~/.kvist/bin/
    cp target/debug/galla-runner ~/.kvist/bin/
    chmod +x ~/.kvist/bin/kvist ~/.kvist/bin/galla-runner

# Install release binaries to user-specific Kvist location (~/.kvist/bin)
install-release:
    cargo build --locked --workspace --release --all-features
    mkdir -p ~/.kvist/bin
    cp target/release/kvist ~/.kvist/bin/
    cp target/release/galla-runner ~/.kvist/bin/
    chmod +x ~/.kvist/bin/kvist ~/.kvist/bin/galla-runner

# Install debug binaries to system-wide location (/usr/local/bin)
install-system:
    cargo build --locked --workspace --all-features
    sudo cp target/debug/kvist /usr/local/bin/
    sudo cp target/debug/galla-runner /usr/local/bin/
    sudo chmod +x /usr/local/bin/kvist /usr/local/bin/galla-runner

# Install release binaries to system-wide location (/usr/local/bin)
install-release-system:
    cargo build --locked --workspace --release --all-features
    sudo cp target/release/kvist /usr/local/bin/
    sudo cp target/release/galla-runner /usr/local/bin/
    sudo chmod +x /usr/local/bin/kvist /usr/local/bin/galla-runner

test:
    kvist vendor
    cargo nextest run --locked --workspace --all-features --test-threads num-cpus

audit:
    cargo deny --all-features --locked check advisories bans licenses sources
