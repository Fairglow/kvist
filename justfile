# Optional convenience recipes using only Cargo and Rustup.
#
# `just` itself is optional; the equivalent Cargo commands are documented in
# README.md. The default recipes do not depend on cargo-nextest, cargo-outdated,
# unbuffer, nightly Rust, or generated log files.

all: format lint build test release

build:
    cargo build --locked --workspace

format:
    cargo fmt --check

lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

msrv:
    cargo +1.85.0 test --locked --workspace

reformat:
    cargo fmt

release:
    cargo build --locked --workspace --release

test:
    cargo nextest run --locked --workspace --test-threads num-cpus

wine:
    @echo "error: Windows support is deferred; Kvist currently supports Linux only" >&2
    @exit 1

completions:
    cargo run --locked -p kvist -- completions bash > /dev/null
    cargo run --locked -p kvist -- completions zsh > /dev/null
    cargo run --locked -p kvist -- completions fish > /dev/null
