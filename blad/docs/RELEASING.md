# Maintaining the Blad release

The registry package is `kvist-blad`; its executable and library are `blad`.
This repository currently prepares version 0.3.0, not an already published
release. Publish only this package. No other workspace package needs releasing.

## Local gates

Run from the repository root with the locked dependency snapshot available:

```sh
cargo fmt -p kvist-blad --check
cargo clippy --offline --locked -p kvist-blad --all-targets --all-features -- -D warnings
cargo test --offline --locked -p kvist-blad --no-default-features
cargo test --offline --locked -p kvist-blad --features sessions
cargo +1.95.0 test --offline --locked -p kvist-blad --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked -p kvist-blad --all-features --no-deps
python3 blad/ci/verify_package.py --offline
```

The Python 3.12+ verifier builds the actual archive, extracts it outside the
repository, tests both feature modes, and release-installs each into an
isolated prefix. Builds use an archive-hash-specific target directory to avoid
reusing another candidate's binaries. It checks target identity, required payload, absence of peer
packages and basic installed behavior. Linux tests require `setsid` and `stty`.
Installed smoke checks compare plain file output, redirected output and stdin
snapshots. The extracted test suite includes actual `cat`/`head` pipelines,
TTY routing, automatic reload, and nonblocking diagnostic-log failures.
Only for a local uncommitted candidate, add `--allow-dirty`; never use that
option for publication. Generated archives are under Cargo's target directory.

The independent Blad workflow runs stable and MSRV checks without requiring
Kvist's sandbox/toolchain provisioning. Both configurations remain supported:
an optional integration feature is still part of the published source.
Dependency policy is in the root `deny.toml`; refresh advisories on the
authorized host/CI before release rather than treating a cached check as fresh.
Performance characterizations are opt-in observations, not timing assertions:

```sh
cargo test --offline --locked -p kvist-blad --test policies --test mermaid_backend -- --ignored --nocapture --test-threads=1
```

## Publication prerequisites

Record exact-revision human acceptance, independent observations/comparison,
security findings and any unresolved limitations. Completed engineering tasks
are not approval. Resolve the retained input-policy and key-precedence
arbitration findings explicitly; do not rewrite historical evidence as accepted.

### Candidate assessment (2026-10-25)

Blad has useful first-release scope for ordinary local Markdown on Linux:
tabbed reading, precise search/links, text math/diagrams, automatic save reload,
bounded live stdin paging and finite dump/redirected-output pipelines.
Do not market it as an unlimited-stream or giant-file pager,
a full Markdown/TeX/Mermaid implementation or a cross-platform viewer.
Automatic reload retains numeric rendered offsets, not source paragraphs;
inactive file tabs refresh when revisited. Terminal stdout automatically pages
stdin through a separate controlling terminal; `--dump` opts out. Finite
snapshots still wait for EOF. The live source ceiling is 16 MiB, and EOF leaves
the viewer open. Release evidence must include real pipe/PTY streaming tests
in both feature modes, including exact limits, cancellation and final fidelity.

The keybinding follow-up reserves `?` for help, removes `p` and link mode,
and relies solely on automatic reload; normal-mode Ctrl+R starts regex search,
and search-prompt Ctrl+R still toggles regex.
Earlier readiness work fixed stale key references,
the rustdoc logging link, and diagnostic initialization that could silently
lose an error or block on a FIFO. Earlier export-authority, nonblank-kind and
help-key findings have candidate documentation clarifications, not retroactive
human acceptance. The conservative 16 MiB limit and modal key choices remain
explicit release decisions for the maintainer.

The release decision is **a bounded Linux release candidate, not unconditional
publication approval**. Independent observations and final local evidence are
retained in `IMPL.md`; the actual registry, protected publishing environment,
fresh advisory data and exact-revision human acceptance must still be verified.
Run a real-terminal smoke test with the maintainer's terminal/font before
publishing; PTY emulation alone is not that evidence.

Verify that the registry name is still available or controlled by the intended
maintainer, verify the maintainer's crates.io account/email, and review the
unchanged AGPL license and transitive notice obligations. Configure GitHub's
`blad-crates-io` environment with required human reviewers and a narrowly
scoped, expiring environment secret named `BLAD_CRATES_IO_TOKEN`. Do not put a
token in source or logs. A suitably configured crates.io trusted publisher may
replace this token flow later; this workflow does not claim OIDC integration.

The manual `Publish Blad` workflow requires an already accepted clean commit
tagged `blad-vVERSION` and its independently accepted full commit SHA. It checks
the explicit tag ref, expected commit and manifest version, reruns archive and
dependency gates, dry-runs publication, and publishes only `kvist-blad`.
Configure protection before dispatch: declaring an environment name in YAML
does not itself establish required reviewers.

Publishing a version is effectively permanent. After explicit release approval,
update the changelog/date and installation availability note, commit the exact
accepted source and tag it; do not create a tag pointing to uncommitted work.
Dispatch with the matching version and full accepted commit SHA, or run from
that same approved clean tree:

```sh
cargo publish --locked -p kvist-blad --dry-run
cargo publish --locked -p kvist-blad
```

## After publication

Verify version-pinned registry installation with `--locked`, repeat the
document/help/version and terminal smoke checks, and test ordinary unlocked
installation on the acquisition-capable host as a separate dependency-resolution
check. Confirm both default and `--features sessions` installations, docs.rs
on Linux, the public README links and the release notes. Do not report these
registry checks as passed based on a local `--path` installation.

Maintain versions and tags independently of Kvist. Prebuilt release binaries
and additional native platforms are optional follow-up work, not prerequisites
for a source crate installed with Cargo.
