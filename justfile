# Optional convenience recipes using only Cargo and Rustup.
#
# `just` itself is optional; the equivalent Cargo commands are documented in
# README.md. The default recipes do not depend on cargo-nextest, cargo-outdated,
# unbuffer, nightly Rust, or generated log files.

all: format lint build test release

build:
    cargo build --manifest-path engine/Cargo.toml --locked --workspace --all-features

format:
    cargo fmt --manifest-path engine/Cargo.toml --check

lint:
    cargo clippy --manifest-path engine/Cargo.toml --locked --workspace --all-targets --all-features -- -D warnings

msrv:
    cargo +1.94.0 test --manifest-path engine/Cargo.toml --locked --workspace --all-features

reformat:
    cargo fmt --manifest-path engine/Cargo.toml

release:
    cargo build --manifest-path engine/Cargo.toml --locked --workspace --release --all-features

test:
    cargo nextest run --manifest-path engine/Cargo.toml --locked --workspace --all-features --test-threads num-cpus

audit:
    cargo deny --manifest-path engine/Cargo.toml --all-features --locked check advisories bans licenses sources

wine:
    @echo "error: Windows support is deferred; Kvist currently supports Linux only" >&2
    @exit 1

completions:
    cargo run --manifest-path engine/Cargo.toml --locked -p kvist -- completions bash > /dev/null
    cargo run --manifest-path engine/Cargo.toml --locked -p kvist -- completions zsh > /dev/null
    cargo run --manifest-path engine/Cargo.toml --locked -p kvist -- completions fish > /dev/null
