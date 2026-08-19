# Optional convenience recipes using only Cargo and Rustup.
#
# `just` itself is optional; the equivalent Cargo commands are documented in
# README.md. The default recipes do not depend on cargo-nextest, cargo-outdated,
# unbuffer, nightly Rust, or generated log files.

all: format lint build test release

build:
    cargo build --locked

format:
    cargo fmt --check

lint:
    cargo clippy --locked --all-targets -- -D warnings

msrv:
    cargo +1.85.0 test --locked

reformat:
    cargo fmt

release:
    cargo build --locked --release

test:
    #cargo test --locked
    cargo nextest run --locked --test-threads num-cpus

wine:
    cargo build --locked --target x86_64-pc-windows-gnu
    cargo test --target x86_64-pc-windows-gnu
