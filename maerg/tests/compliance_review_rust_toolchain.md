<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Pinned Rust Toolchain — Provisioning, Recording, and Enforcement (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `rust-toolchain-compliance-review`
**Scope:** The source-derived Rust toolchain behavior — pin source and precedence,
channel-explicit resolution, host provisioning, versioned manifest recording, and
fail-closed enforcement — compared against `REQ-RUST-TOOLCHAIN`, the toolchain
guarantees in the consumer contract, and the ADR-0012 design. The recorded
workspace-root toolchain manifest (`.kvist/rust-toolchain.json`) is the artifact
under independent confirmation.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is the
fourth and final stage of the `rust-toolchain` chain (write-tests → implement-code →
security-audit → compliance review). The implementer and the security auditor did
not author this record. Per the runbook, this reviewer did not read
`maerg/src/**` implementation source, dependency manifests, or Git history as
evidence; observed behavior is taken from the bounded in-scope test assertions and
the recorded manifest listed below, and from the bounded chain execution evidence
of the immediately preceding stages where the record is silent.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read only
the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the newly produced
`IMPL.md` and bounded test evidence; the nearest ancestor component `CONTRACT.md`
(none — `maerg` is the root component); and `ROOT_CONTRACT.md`. Each clause is
classified **compliant**, **mismatched**, **approved-deferred**, or
**underspecified**, with component path, stable locator, intended text, observed
behavior/evidence, severity, and owner.

**Read (permitted intent evidence):**
- `maerg/REQUIREMENTS.md` — `REQ-RUST-TOOLCHAIN` (~line 701) and its recorded
  "Current implementation status" note; the `REQ-LANGUAGE-SUPPORT` Rust authoring
  sentences.
- `maerg/CONTRACT.md` — `Provided interfaces` command surface (`toolchain
  [PROJECT_DIR]`); the ADR-0012 provisioning sentence; the `Behavioral guarantees`
  toolchain paragraph; the native-formats entry for `.kvist/rust-toolchain.json`;
  the authoring System/Toolchain grant paragraph.
- `maerg/DESIGN.md` — `Pinned Rust toolchains and host provisioning (ADR-0012)`
  (pin source, channel-explicit resolution, provisioning step, manifest,
  enforcement on use).
- `maerg/IMPL.md` — checked for a section describing the toolchain provisioning,
  pin parsing, channel validation, manifest, or fail-closed enforcement. It contains
  none (see F-1).
- `ROOT_CONTRACT.md` — `Non-negotiable architecture` (distinct authority phases; a
  separate compliance reviewer verifies implemented behavior) and `Change and
  compliance rules` (record intent-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact).

**Read (permitted bounded in-scope test evidence):**
- `maerg/tests/offline_cargo_verification_e2e.rs` —
  `offline_cargo_verification_uses_pinned_toolchain` and
  `offline_cargo_verification_uses_pinned_nightly_toolchain` (both self-skipping;
  drive `maerg::toolchain::ensure_toolchain` and `run_offline_cargo_verification`
  through the real production path in a network-denied bubblewrap sandbox).
- `maerg/tests/sandbox_toolchain.rs` — the sandboxed Rust toolchain availability
  tests (`rustc`/`cargo`/`clippy` present in the sandbox; authoring inventory and
  offline builds).
- `maerg/tests/artifact_recovery.rs` — the assertion that a pre-existing
  `.kvist/rust-toolchain.json` is backed up (and moved out of the live `.kvist`) on
  clean-slate approval.

**Read (permitted execution evidence of record — the recorded artifact and bounded
chain logs):**
- The recorded root manifest `.kvist/rust-toolchain.json` (workspace root).
- `.kvist-attempts/rust-toolchain-security-audit.jsonl` (the immediately preceding
  stage's recorded execution).

**Not read:** `maerg/src/**` implementation source, `Cargo.toml`/`Cargo.lock`, Git
history or diffs, prior compliance conclusions, and peer or parent implementation
artifacts — including `skott/src/**`, `skott/tests/**`, `galla/src/**`, and
`sav/src/**`. This reviewer executed no tests, builds, linters, or `cargo`
invocations; the chain's test suites are run automatically by Kvist in the
verification sandbox after this turn, and no pass/fail count from that run is
asserted here. Where a file digest is not independently recomputed, it is attributed
to the chain's recorded revision digests rather than recomputed here.

## Method

Every normative obligation in `REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` was compared
against the corresponding contract guarantee (`CONTRACT.md` `Provided interfaces`
and `Behavioral guarantees`), the ADR-0012 design claim (`DESIGN.md`), and the
observed behavior recorded in the bounded in-scope test evidence and the recorded
root manifest. The two e2e tests in `offline_cargo_verification_e2e.rs` are the
executable statement of observed behavior for the happy path; the on-disk manifest
is the direct observation of the recording obligation. `IMPL.md` was checked as
the implementation-observed-behavior source and found to be silent (F-1).

## Satisfied clauses (compliant)

**C-1 — The project pin is authoritative when present; the rustup default is used
and recorded as `default` otherwise.**
`REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` ("When a project root contains
`rust-toolchain.toml` (preferred) or `rust-toolchain`, its channel MUST be the
authoritative toolchain selection; otherwise the rustup default MUST be used and
recorded as such."). Observed: the e2e tests write a `rust-toolchain.toml` pin and
assert the recorded `manifest.channel` equals the pinned channel; the recorded
root manifest carries `channel: "default"`, and no `rust-toolchain.toml` or
`rust-toolchain` pin file exists at the workspace root — so the root default is
used and recorded as `default`, exactly as the clause requires. The `nightly`
variant proves the pin overrides the host default. Compliant.

**C-2 — Provisioning is a distinct, host-authorized step performed outside the
sandbox; it resolves, installs when absent, validates the layout, and records a
versioned manifest.**
`REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` (provisioning MUST be "a distinct
host-authorized step performed outside the sandbox (ADR-0012)"); `CONTRACT.md`
`Provided interfaces` (`toolchain` "is a host-authorized provisioning step
(ADR-0012): it resolves the project's pinned Rust toolchain … installs the pinned
channel via `rustup` when absent … validates the toolchain layout, and records the
versioned toolchain manifest under `.kvist/` … It is the only supported toolchain
change path and never runs inside a sandbox."); `DESIGN.md` ADR-0012 (provisioning
step). Observed: the e2e tests invoke `maerg::toolchain::ensure_toolchain` on the
host before the sandboxed verification and assert a manifest is produced; the
recorded root manifest is present under `.kvist/`. Compliant.

**C-3 — The recorded manifest is versioned, carries the channel, toolchain root,
exact cargo path, and cargo content digest, and matches the declared schema.**
`REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` ("records a versioned manifest under
`.kvist/` carrying the channel, toolchain root, exact cargo path, and cargo content
digest"); `CONTRACT.md` native-formats (".kvist/rust-toolchain.json: `schema_version:
1` with the pinned channel, toolchain root, cargo path, cargo content digest, and
provisioning timestamp"). Observed (direct on-disk record, workspace root):
`schema_version: 1`, `channel: "default"`, `toolchain_root:
/home/stefan/.rustup/toolchains/stable-x86_64-unknown-linux-gnu`, `cargo_path:
<same>/bin/cargo`, `cargo_digest:
sha256:a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af`,
`provisioned_at_unix_secs: 1793133244`. The record is schema-conformant and the
`cargo_path` sits under `toolchain_root/bin/cargo`, consistent with the
layout-validation claim. Compliant.

**C-4 — Verification never installs, upgrades, or modifies a toolchain; it
consumes the already-provisioned one.**
`REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` ("Builds and verification MUST NEVER
install, upgrade, or modify a toolchain; they MUST only consume an already-provisioned
one."); `CONTRACT.md` `Provided interfaces` ("Builds and verification never install,
upgrade, or modify a toolchain; they consume the provisioned one and fail closed
when it is absent or has drifted from the recorded manifest."). Observed: the
verification phase in the e2e tests runs offline (`cargo test --offline --locked`
inside the network-denied sandbox) and consumes the toolchain the provisioning step
recorded; the test contains no `rustup toolchain install`/upgrade/modify call in the
verification phase. Compliant.

**C-5 — Verification resolves the toolchain channel-explicitly, independent of the
process working directory, and not via the ambient default.**
`REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` ("Verification MUST resolve the toolchain
channel-explicitly (independent of the process working directory and ambient rustup
overrides)"); `CONTRACT.md` `Behavioral guarantees` ("Verification resolves the
toolchain channel-explicitly, independent of the process working directory, and
fails closed …"); `DESIGN.md` (`rustup which cargo --toolchain <channel>`). Observed:
the pinned-toolchain test provisions a toolchain and then runs offline verification
that builds against that exact pinned channel; the `nightly` test goes further and
asserts the recorded `cargo_path` differs from the host's active-default cargo path
whenever the active channel is not `nightly` — directly proving the resolution is
off-default and driven by the pin, not the ambient toolchain. The tests operate on a
throwaway project directory inside the git worktree, exercising the
working-directory-independence aspect. Compliant.

## Discrepancies retained for human arbitration

**F-1 — `IMPL.md` does not describe the ADR-0012 toolchain behavior, so the
source-blind review cannot anchor to implementation-observed behavior in the
implementation record.**
Classification: **underspecified**. Severity: **medium**. Owner: **project
architect**.

Intended obligation: `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md`
describes observed implementation behavior"; "A clean-slate documenter and a
separate compliance reviewer must verify implemented behavior") and
`REVIEW_RUNBOOK.md` ("the newly produced `IMPL.md` and bounded test evidence" are
permitted compliance-review inputs).

Observed: a search of `maerg/IMPL.md` for the toolchain provisioning concepts
(`toolchain`, `provision`, `channel`, `ADR-0012`, `ensure_toolchain`,
`rust-toolchain`, `rustup toolchain`) surfaces no section describing the pinned
toolchain's pin parsing, channel validation, host provisioning, versioned manifest
recording, or fail-closed enforcement. The only toolchain-adjacent `IMPL.md`
content concerns the authoring System/Toolchain inventory (`/rust/toolchains/N`
preparation) and the runtime-bin staging helper — not the ADR-0012 verification
topology this review covers.

Analysis: this is not an intent-to-implementation *mismatch* (no observation
contradicts the contract); it is an *evidence-gap* for a source-blind reviewer. The
runbook's permitted "newly produced `IMPL.md`" does not describe the behavior under
review, so this record has to rest on the in-scope e2e test assertions and the
recorded manifest alone. That is a defensible basis for the happy path (C-1…C-5) but
it means the "implementation-observed behavior" of the toolchain module is not
independently restated anywhere the reviewer is permitted to read. Retained so the
architect decides whether `IMPL.md` should carry an ADR-0012 observation section.

Recommended arbitration (not applied): add a clean-slate implementation-observation
section to `maerg/IMPL.md` describing the pin parsing, channel validation,
host-provisioning, manifest recording, and fail-closed enforcement behavior (with its
limits), so a later source-blind compliance review can anchor to the record rather
than to test assertions alone. This is an implementation-record change only; no
behavioral change is implied.

**F-2 — The fail-closed *negative* behaviors have no in-scope executable test in
`maerg/tests/`.**
Classification: **underspecified** (in-scope executable evidence absent; no
contradictory in-scope observation). Severity: **medium**. Owner: **project
architect**.

Intended text: `REQUIREMENTS.md` `REQ-RUST-TOOLCHAIN` ("Channel parsing MUST fail
closed on malformed, oversized, or link-like pin files, and channel syntax MUST be
validated to the rustup-supported forms …"); ("MUST fail closed with an actionable
message when the pinned toolchain is absent or the recorded manifest no longer
matches the on-disk toolchain."); `CONTRACT.md` `Behavioral guarantees`
("fails closed with an actionable message when the pinned toolchain is absent or
the manifest no longer matches the on-disk toolchain").

Observed: the in-scope `maerg/tests/` files exercise only the *happy* path — pin
precedence, manifest recording, channel-explicit off-default resolution (C-1…C-5) —
plus the manifest backup on clean-slate approval. There is no in-scope `maerg/tests/`
case that drives a malformed/oversized/link-like pin, an absent pinned toolchain, or
a manifest/no-longer-matching-the-on-disk-toolchain drift and asserts the fail-closed
result. The only such assertions the reviewer could locate trace to the immediately
preceding security-audit stage, whose recorded execution names a deliverable
(`tests/toolchain_pin_security.rs`) and findings doc that live in the **peer `skott`**
component (see O-1) — outside the permitted scope of a `maerg` source-blind review.

Analysis: the *requirement* and *contract* are clear about the negative behaviors;
the *design* (ADR-0012) specifies them; and the implementation status note claims
they are "evidenced by unit tests plus end-to-end tests." But the in-scope,
source-visible executable evidence for the negative paths is not present in
`maerg/tests/`, and the record's implementation-observation source is silent (F-1).
A source-blind reviewer therefore cannot independently confirm the negative
fail-closed behavior from in-scope artifacts; it is attributed to the bounded
preceding-stage execution evidence, not independently re-observed here. This is
retained so the architect confirms whether the negative behaviors are covered by the
maerg module's own unit tests and/or a maerg in-scope integration test, or records
that cross-component (peer `skott`) coverage is the accepted evidence.

Recommended arbitration (not applied): either (a) add a `maerg` in-scope regression
that drives a malformed pin, an absent pinned toolchain, and a manifest drift and
asserts the actionable fail-closed result (so the negative behaviors are evidenced
within the component under review), or (b) explicitly record that the ADR-0012
negative-path coverage is intentionally provided by the peer `skott` component and
note that boundary in the intent/record. Both are test/record changes; no behavior
change is implied.

## Observations (informational, no reclassification required)

**O-1 — The preceding security-audit stage's deliverables live in the peer `skott`
component, not `maerg`.** The recorded execution of
`rust-toolchain-security-audit` names implementation files under `skott/src/`,
`galla/src/`, and `sav/src/` and a test deliverable `tests/toolchain_pin_security.rs`;
the findings doc is present at `skott/.kvist/audit-findings-toolchain-security.md`
(and is scoped in its header to "Component: skott … `src/rust_environment/`"), not
under `maerg/.kvist/`. The `maerg` e2e tests exercise the `maerg::toolchain::ensure_
toolchain` public API, so the pin-resolution logic the audit covered is shared with
the audited peer source. Informational only: it raises a component-boundary question
about which component owns the ADR-0012 provisioning behavior and where its
executable evidence should live for a `maerg` compliance review. It is not read as
evidence here (peer artifacts are out of scope); it is noted because it is the reason
the negative-path evidence in F-2 is out-of-scope for this review. No behavior
change is implied.

**O-2 — The recorded root manifest is consistent with the "no pin → default" rule.**
No `rust-toolchain.toml` or `rust-toolchain` pin file exists at the workspace root,
and the recorded `.kvist/rust-toolchain.json` carries `channel: "default"`. The
recorded artifact is therefore semantically consistent with C-1's default-recording
clause. Informational confirmation of the specific recorded state.

## Bounded-evidence limitations (context, not findings)

- The two in-scope e2e tests are self-skipping (they require a built sandbox runner,
  a bubblewrap backend, a git worktree, and, for the `nightly` case, an installed
  nightly toolchain). This review reads them as the executable statement of the happy
  path; whether they execute or skip, and any pass/fail count, is determined when
  Kvist runs the suite in the verification sandbox after this turn, and is not
  asserted here.
- The negative fail-closed behaviors (absent pinned toolchain, manifest drift,
  malformed/oversized/link-like pins) are not independently re-observed here: the
  reviewer is source-blind (so `maerg/src/toolchain.rs` and its in-module tests are
  out of scope) and the maerg in-scope test files do not exercise the negative paths
  (F-2).
- The file digests of the intent documents and the recorded manifest are attributed
  to the chain's recorded revision digests, not recomputed by this reviewer.
- The peer `skott` test file, the peer findings doc, and the preceding audit's
  named peer source files are outside the permitted scope of a `maerg` source-blind
  review and are cited only as a boundary observation (O-1), not as evidence.

## Verdict

The **happy-path** toolchain behavior — pin source and precedence, host provisioning
as a distinct step outside the sandbox, the versioned `.kvist/rust-toolchain.json`
record (channel, root, cargo path, cargo digest, timestamp), and channel-explicit,
off-default, working-directory-independent consumption by offline verification —
**matches the declared `REQ-RUST-TOOLCHAIN` requirement and the contract** (C-1…C-5).
The **recorded root manifest is independently confirmed**: it is schema-conformant to
the contract's declared `.kvist/rust-toolchain.json` format and semantically
consistent with the "no pin at the root → rustup default used and recorded as
`default`" clause (C-1, C-3, O-2).

Two items are retained for human arbitration rather than certified or silently
closed:
1. **F-1 (underspecified, medium):** `IMPL.md` does not describe the ADR-0012
   toolchain behavior, so the source-blind review cannot anchor to
   implementation-observed behavior in the implementation record and must rest on the
   in-scope test assertions and the recorded manifest.
2. **F-2 (underspecified, medium):** the fail-closed *negative* behaviors (malformed,
   oversized, link-like pins; absent pinned toolchain; manifest drift) have no in-scope
   executable test in `maerg/tests/`; the only such evidence the reviewer could locate
   belongs to the peer `skott` component and is out of scope here.

One boundary observation (O-1) records that the preceding security-audit stage's
deliverables live in the peer `skott` component. This review does not certify
compliance, does not modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or
`IMPL.md`, and defers each retained item to the project architect for explicit
arbitration.
