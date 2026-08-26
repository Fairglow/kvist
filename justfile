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
    cargo nextest run --locked --test-threads num-cpus

wine:
    cargo build --locked --target x86_64-pc-windows-gnu
    @mkdir -p /opt/target/wine/drive_c/windows
    @echo "@echo off" > /opt/target/wine/drive_c/windows/git.bat
    @echo "Z:\\usr\\bin\\git %*" >> /opt/target/wine/drive_c/windows/git.bat
    @if which jj >/dev/null 2>&1; then \
        echo "@echo off" > /opt/target/wine/drive_c/windows/jj.bat; \
        echo "Z:\\home\\stefan\\.cargo\\bin\\jj %*" >> /opt/target/wine/drive_c/windows/jj.bat; \
    fi
    WINEARCH=win64 WINEPREFIX=/opt/target/wine WINEDEBUG=-all cargo test --target x86_64-pc-windows-gnu

completions:
    cargo run --locked -- completions bash > /dev/null
    cargo run --locked -- completions zsh > /dev/null
    cargo run --locked -- completions fish > /dev/null
