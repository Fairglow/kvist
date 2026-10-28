# Optional convenience recipes.
#
# `just` itself is optional; the equivalent Cargo commands are documented in
# README.md. Testing uses cargo-nextest; profiling additionally uses Python 3.11+.

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
    python3 tools/cargo_install_binaries.py .kvist/cargo-build.log target/debug

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
    python3 tools/cargo_install_binaries.py .kvist/cargo-release.log ~/.kvist/bin target/release

# Print the target directory on the host
target:
    cargo metadata --format-version 1 --no-deps | python3 -c "import sys, json; print(json.load(sys.stdin)['target_directory'])"

test:
    kvist vendor
    cargo nextest run --locked --workspace --all-features --test-threads num-cpus --no-fail-fast

test-profile:
    kvist vendor
    NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 python3 tools/profile_tests.py -- cargo nextest run --offline --locked --workspace --all-features --test-threads num-cpus --no-fail-fast --message-format libtest-json-plus --message-format-version 0.1

audit:
    cargo deny --all-features --locked check advisories bans licenses sources
