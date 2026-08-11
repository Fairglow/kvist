# Just recipes for build actions
alias old := outdated

all: format build test

build: lint
    time unbuffer cargo build 2>&1 | tee build.log

# Check formatting without modifying files (useful in CI).
format:
    cargo fmt --check

lint:
    unbuffer cargo clippy 2>&1 | tee lints.log

outdated:
    cargo outdated --depth=1

# Format using nightly rustfmt (enables unstable options in rustfmt.toml).
# The stable compiler is unaffected — only the formatter binary is from nightly.
reformat:
    cargo fmt

release:
    cargo build --release

test:
    unbuffer cargo nextest run --test-threads num-cpus 2>&1 | tee test.log
