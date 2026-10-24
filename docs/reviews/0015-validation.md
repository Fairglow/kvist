# ADR 0015 integration execution evidence

Recorded by the implementation orchestrator on 2026-10-24, after the final
native-selector repair in `5cb86cb`. This is execution evidence, not an
independent compliance judgment, Kvist receipt, acceptance, or waiver.

## Final gates

All Cargo commands used `CARGO_TARGET_DIR=/opt/target`; build, test, lint and
documentation commands used `--offline --locked`.

| Command | Observed result |
| --- | --- |
| `cargo test --workspace --no-fail-fast --quiet` | 1746 passed, zero failed, 14 existing ignored, 104 successful target summaries |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo build --release --workspace --bins` | Passed |
| `cargo doc --workspace --no-deps` | Passed with existing maerg warnings |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p sav -p skott` | Passed |
| `git diff --check` | Passed |

The last complete workspace run finished at 2026-10-24T00:00:55Z.
The 14 ignored cases were not newly skipped. A strict whole-workspace rustdoc
pass is not claimed: existing maerg authoring/pager link warnings remain.
Aggregate success does not establish individual clause coverage, operating
system enforcement, crash durability, all concurrency interleavings, or remote
service qualification.

The earlier 1743-pass execution cited by the independent comparisons predates
three added selector regressions. The later complete Sav package and final
workspace runs passed. The selector rejection test failed before repair;
the exact 256-byte acceptance and invalid-before-connection cases now pass.
Earlier tests-first audit repairs also cover private transcript path handling,
Unicode slugs, single final answers, context materialization, cumulative
accounting, redaction boundaries and retained partial/uncertain outcomes.

## Integration checks and delivered limits

Actual release binaries `sav-run`, `skott` and `kvist` loaded the same local
private catalogue without provider inference. Skott listed the external
profile once; Kvist no longer printed misleading empty-model or unrelated
legacy-catalogue advice in shared mode. Local `kvist.toml`, `skott.toml` and
`.kvist/providers.toml` remain ignored and untracked; the registry is mode 0600.
No private configuration was force-added.

A separate PTY smoke exercised inspector guidance, cancellation and detachment
through correlated private socket requests without changing the observed
journal. Native guidance preserves active work; opaque external-agent guidance
is explicitly unsupported. Opaque execution requires terminal-owned host
acknowledgement and is refused for protected/headless combinations. No real
Copilot or other remote inference/qualification was performed.

Protected automatic dependency fulfillment now refuses unsupported/unbound
requests before host effects. It does not equate ready unrelated vendoring
with fulfillment or fall back to host package managers. The normative
automatic-fulfillment target remains unmet; explicit human provisioning is
separate.

## Independent comparison and blocked formal gates

Separate source-only contexts derived component `IMPL.md` records. Separate
source-blind contexts produced:

- [Sav diagnostic comparison](0015-sav-comparison.md)
- [Skott diagnostic comparison](0015-skott-comparison.md)
- [Maerg diagnostic comparison](0015-maerg-comparison.md)

Concrete coupled findings were repaired and re-observed, including the native
selector incompatibility. Existing active-task/bootstrap and executor-owner
wording was clarified under the approved steering/consolidation direction.
Broader discrepancies and evidence gaps were retained, not silently converted
into approved deferrals or compliance.

`kvist doctor .` still reports a pre-existing Skott `lifecycle-write-tests`
completion/update timestamp inconsistency, invalid project state with VCS
readiness not checked, and an unavailable Kvist-managed Cargo path. Successful
host-toolchain gates do not repair those prerequisites. Intent baselines were
not automatically accepted, unrelated task history was not rewritten, and
formal compliance tasks remain blocked for readiness repair and explicit
human arbitration of the documented discrepancies.
