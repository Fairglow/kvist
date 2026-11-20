<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Per-Language Offline Verification — Provisioning and Evidence (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `language-e2e-compliance-review`
**Scope:** The source-derived per-language provisioning and offline-verification
behavior — the declared per-language strategies, the shared offline language
topology, the `kvist vendor` dispatch, and the per-language end-to-end
evidence — compared against `REQ-LANGUAGE-SUPPORT` in `REQUIREMENTS.md`, the
language-support guarantees in the consumer contract, and the ADR-0013 design
record. This is the compliance-review stage of the per-language offline
verification chain (write-tests → implement-code → security-audit →
compliance review).
**Reviewer stance:** Source-blind, independent compliance reviewer. The
implementer and the security auditor did not author this record. Per the
runbook, this reviewer did not read `maerg/src/**` implementation source as
evidence; observed behavior is taken from the bounded in-scope test assertions
listed below and from the intent documents, and from the component's
implementation record `IMPL.md` to the extent it describes the behavior under
review.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read
only the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the newly
produced `IMPL.md` and bounded test evidence; the nearest ancestor component
`CONTRACT.md` (none — `maerg` is the root component); and `ROOT_CONTRACT.md`.
Each clause is classified **compliant**, **mismatched**, **approved-deferred**,
or **underspecified**, with component path, stable locator, intended text,
observed behavior/evidence, severity, and owner.

**Read (permitted intent evidence):**
- `maerg/REQUIREMENTS.md` — `REQ-LANGUAGE-SUPPORT` (header line 739) and its
  recorded "Current implementation status" note (line 774 onward); the
  immediately preceding `REQ-RUST-TOOLCHAIN` status note as context for the
  Rust closed topology.
- `maerg/CONTRACT.md` — the `Behavioral guarantees` language-support paragraph
  (header line 1211; "Language support is declared per language with executable
  evidence" line 1281 onward, including the per-language canonical commands and
  the non-vendored/JVM-and-Ruby/unsupported-language sentences); the `Native Go
  module cache` section; the `Native Rust language strategy` section; the
  `Native package-manager acquisition` and `Non-Rust offline cache preparation`
  sections; the `vendor` provisioning sentence (line 838 onward, ADR-0020/
  ADR-0013).
- `maerg/DESIGN.md` — the `Language support, per-language provisioning, and
  evidence (ADR-0013)` section (header line 963) including the shared offline
  language topology, the per-language profiles, and the evidence-status
  sentences (lines 1057–1058).
- `maerg/IMPL.md` — checked for a section describing the per-language offline
  verification behavior (see F-3/F-4; it contains only a one-line dispatch
  reference, not an observation section for this topology).
- `maerg/README.md` — the supported-language table (header line 685 onward),
  the `Intended support order and evidence` and `Declared limitations`
  sections, and the `Explicitly unsupported` section. The requirement itself
  obligates that "supported languages and their limitations MUST be documented
  in the component README," so the README is the artifact checked for that
  specific documentation clause.
- `ROOT_CONTRACT.md` — `Change and compliance rules` (record intent-to-
  implementation discrepancies for explicit arbitration; do not silently alter
  either artifact) and `Non-negotiable architecture`.

**Read (permitted bounded in-scope test evidence):**
- `maerg/tests/language_offline_e2e.rs` (read in full) — the per-language
  offline end-to-end tests: `go_offline_verification_builds_and_tests_denied_
  network`, `javascript_offline_verification_builds_and_tests_denied_network`
  (npm), `javascript_pnpm_offline_verification_builds_and_tests_denied_network`
  (pnpm), `python_offline_verification_builds_and_tests_denied_network` (pip),
  `python_uv_lock_offline_verification_builds_and_tests_denied_network` (uv),
  `c_offline_verification_builds_and_tests_denied_network` (Conan), and
  `c_vcpkg_offline_verification_builds_and_tests_denied_network` (vcpkg). All
  self-skip when the live sandbox or the language toolchain is unavailable.
- `maerg/tests/offline_cargo_verification_e2e.rs` — `offline_cargo_
  verification_builds_tested_project_denied_network` (the Rust closed-topology
  real-project build/test e2e) and the two pinned-toolchain tests.
- `maerg/tests/sandbox_toolchain.rs` — the Rust authoring tests
  (`rust_authoring_has_host_inventory_private_defaults_and_offline_builds`,
  `rust_authoring_builds_workspace_members_with_readonly_provider_context`,
  `generic_authoring_preserves_environment_without_rustup_state`) and the
  sandboxed Rust toolchain availability tests.
- `maerg/tests/language_vendoring_integration.rs` — the cheap catalogue/
  presence checks for the native (non-Rust) strategies (no executed offline
  build).
- `maerg/tests/test_policy.rs` — the `[test_policy]` digest/command-set checks
  relevant to the C/C++ approved-test-command path.

**Consulted as a referenced design record (outside the strict runbook read
set; noted for transparency):** `docs/decisions/0013-per-language-offline-
verification.md`. `CONTRACT.md` and `DESIGN.md` both cite ADR-0013 as the
governing design for the shared offline language topology, and the requirement
references that topology by name. The ADR was read to check the design claim
the requirement and contract point to; where this record cites the ADR it is
cited as a design/decision record, and the discrepancy it produces is retained
for arbitration.

**Not read / not relied upon:** `maerg/src/**` implementation source;
`Cargo.toml`/`Cargo.lock`; Git history or diffs; and prior compliance
conclusions. The immediately-preceding stage's record
(`security_audit_language_offline.md`) was consulted only as bounded context
for the closed-profile enumeration and network-denial mechanism, and is **not**
treated as a compliance conclusion or as evidence of compliance for this
record. This reviewer executed no tests, builds, linters, or `cargo`
invocations; the chain's test suites are run automatically by Kvist in the
verification sandbox after this turn, and no pass/fail count from that run is
asserted here.

## Method

Every normative obligation in `REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` was
compared against the corresponding contract guarantee (`CONTRACT.md`), the
ADR-0013 design claim (`DESIGN.md` and the ADR-0013 record), and the observed
behavior recorded in the bounded in-scope test evidence. The per-language e2e
tests are the executable statement of observed behavior for each vendored
language's happy path; the catalogue/presence tests and the `IMPL.md` record
are the cheap-check and implementation-observation sources. `IMPL.md` was
checked as the implementation-observed-behavior source for this topology (see
F-4). Where the "Current implementation status" note in the requirement asserts
a language is vendored-supported, the note was checked against the named e2e
test names and the e2e tests actually present in `language_offline_e2e.rs`.

## Satisfied clauses (compliant)

**C-1 — The per-language offline verification runs a real, network-denied,
offline build/test inside the Bubblewrap sandbox, asserting a successful run,
for every vendored language the e2e suite covers.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("A language is supported for a phase
only when an end-to-end integration test vendors (or otherwise provisions) a
small real project on the host and runs its real build/test offline inside the
Bubblewrap sandbox, asserting a successful run; legacy language tests MAY
self-skip on hosts without the live sandbox."). Observed: each of the seven
named e2e tests in `language_offline_e2e.rs` provisions a small real project on
the host (`go mod tidy`/`kvist vendor`, `npm ci`/`kvist vendor`, `pip download`
+ venv/`kvist vendor`, `uv lock`+`uv export`/`kvist vendor`, `pnpm install`/
`kvist vendor`, `conan install`/`kvist vendor`, `vcpkg install`/`kvist vendor`)
and then drives the real production path
(`run_offline_language_verification`) inside the bubblewrap sandbox, asserting
a successful exit status plus a language-specific evidence marker (`ok`/`PASS`
for Go, `tests 1`/`pass 1` for Node, `OK`/`Ran 1 test` for Python, `zlib
roundtrip ok` for both C variants). Every test self-skips when the live sandbox
or the language toolchain is unavailable, exactly as the clause permits. The
Rust closed topology is separately evidenced by
`offline_cargo_verification_builds_tested_project_denied_network`. Compliant.

**C-2 — The shared offline language topology is wired into explicit `kvist
vendor` and the shared network-denied verification, with vendoring enforced
and the vendored material mounted read-only under the lock-file digest
identity.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("These strategies are wired into
explicit `kvist vendor` and the shared network-denied verification topology");
`CONTRACT.md` `Behavioral guarantees` ("Every other **vendored** project is
routed to the shared offline language topology (ADR-0013): vendoring is
enforced, the language's vendored material is mounted read-only with the
lock-file digest identity, one disjoint writable scratch is granted … the
network is denied, and the language's canonical offline test command runs");
`DESIGN.md` ADR-0013 (shared topology: enforcement first, read-only mounts at
the lock-file digest identity, one disjoint writable scratch at
`/workspace/scratch`, network denied). Observed: the e2e tests call
`vendor_project` (the `kvist vendor` dispatch) to readiness before verification
and then run the offline language verification; the C/C++ tests additionally
assert the provisioned artifacts exist (`.kvist/conan-build/conan_toolchain.
cmake`, `.kvist/vendored-vcpkg/installed` + `…/scripts/buildsystems/vcpkg.
cmake`) and the pnpm test asserts the vendored content-addressable store is
non-empty, which is consistent with the enforced/read-only/digest-identity
wiring described by the contract and design. Compliant (the enforcement
ordering and mount identity are contract/design claims corroborated by the
in-scope e2e preconditions; the runner-level mechanism is not re-proven here).

**C-3 — The per-language provisioning strategies are as declared: Rust native
read-only Cargo sources; Go native read-only module cache; Python and
JavaScript installed environments / project-native caches; C/C++ native Conan
home or installed vcpkg root.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("Rust uses native read-only Cargo
sources and pinned installed toolchains. Go uses the native read-only module
cache. Python and JavaScript retain useful installed environments/project-native
caches; C/C++ uses the native Conan home or installed vcpkg root.");
`CONTRACT.md` `Native Go module cache` (Go uses `GOMODCACHE`, `go mod
download`, and does not create or refresh project `vendor/`) and the language-
support paragraph; `DESIGN.md` ADR-0013 per-language profiles. Observed: the Go
e2e provisions through the native host module cache and explicitly asserts
`!root.join("vendor").exists()` ("native preparation must not copy module
sources"), matching "Go uses the native read-only module cache"; the Python
tests provision a virtualenv and import from it; the JavaScript tests provision
`node_modules`/the pnpm store; the C tests provision the Conan home and the
vendored vcpkg root. Compliant. (The ADR-0013 Go description diverges from this
declared strategy and is retained as F-2.)

**C-4 — The canonical offline test command is used per language, and an
approved project test command drives only C/C++ (the project-defined build
system).**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("the language's canonical offline
test command (or, for C/C++ only, the approved `[test_policy]` command)");
`CONTRACT.md` `Behavioral guarantees` ("Go: `go test -mod=readonly ./...`;
JavaScript: `node --test`; Python: `python3 -m unittest -v` … C/C++: the
approved `[test_policy]` command (the build system is project-defined)");
`DESIGN.md` ADR-0013. Observed: the e2e helper passes `test_command = None` for
the canonical-command languages (Go, JavaScript, Python) and
`Some([make, test])` for both C variants, matching the declared
canonical/approved split. Compliant.

**C-5 — Both `requirements.lock.txt` (pip) and `uv.lock` (uv) locked Python
projects are vendored-supported and evidenced end to end, and the `uv.lock`
path exports its exact resolved graph with `uv export --locked`.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("Both `requirements.lock.txt` (pip)
and `uv.lock` (uv) locked Python projects vendoring and verify offline; a
`uv.lock` project is provisioned by exporting its exact resolved graph with
`uv export --locked` …"); `CONTRACT.md` and `DESIGN.md` ADR-0013 (Python
profile, both lock forms, `uv export --locked`). Observed:
`python_offline_verification_builds_and_tests_denied_network` uses a pinned
`requirements.lock.txt` (pip) project, and
`python_uv_lock_offline_verification_builds_and_tests_denied_network` generates
a `uv.lock` on the host (`uv lock`) and pulls a transitive dependency,
exercising the export-graph path. Both are present and self-skipping.
Compliant.

**C-6 — JavaScript is vendored-supported for npm (`package-lock.json`) and
pnpm (`pnpm-lock.yaml`), and the pnpm path vendors the content-addressable
store.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("JavaScript is vendored-supported for
npm (`package-lock.json`) … and pnpm (`pnpm-lock.yaml`); the pnpm path vendors
the content-addressable store"); `CONTRACT.md` and `DESIGN.md` ADR-0013
(JavaScript profile). Observed: `javascript_offline_verification_builds_and_
tests_denied_network` is a zero-dependency `package-lock.json` (npm) project and
`javascript_pnpm_offline_verification_builds_and_tests_denied_network` is a
`pnpm-lock.yaml` project with a transitive dependency that asserts the vendored
pnpm store is non-empty before verification. Compliant for the npm and pnpm
sub-variants. (The requirement's additional claim for yarn — `yarn.lock` — is
not supported by the e2e evidence and is retained as F-1.)

**C-7 — C/C++ is vendored-supported for both Conan (`conanfile`) and vcpkg
(`vcpkg.json`), and the vcpkg path vendors the whole vcpkg root.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("C/C++ is vendored-supported for
Conan (`conanfile`) and for vcpkg (`vcpkg.json`); the vcpkg path vendors the
whole vendored vcpkg root"); `CONTRACT.md` `vendor` sentence (line 838 onward,
C/C++ project-local Conan home and generated build files / vcpkg) and the
language-support paragraph; `DESIGN.md` ADR-0013 (C/Conan and C/vcpkg
profiles). Observed: `c_offline_verification_builds_and_tests_denied_network`
provisions the Conan home and generated CMake toolchain, and
`c_vcpkg_offline_verification_builds_and_tests_denied_network` provisions
`.kvist/vendored-vcpkg` (asserting both the `installed/` ports and the CMake
toolchain file) and resolves zlib from it. Both are present and self-skipping.
Compliant.

**C-8 — Evidence distinguishes cheap catalogue/presence checks from executed
offline builds, and does not overclaim source-integrity attestation.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("Evidence MUST distinguish cheap
catalogue/presence checks from executed offline builds and source-integrity
attestation."). Observed: the executed-offline-build evidence is the e2e suite
(which asserts a real build/test success and a language-specific marker), and
the cheap catalogue/presence evidence is `language_vendoring_integration.rs`
(which checks vendoring reports, mount identities, and lock-drift refusal
without executing an offline build). The contract is honest that the lock-file
digest is a catalogue and does not "prove package-by-package completeness"
(`Non-Rust offline cache preparation`), i.e., no source-integrity attestation
is claimed where none is performed. The two evidence tiers are kept distinct.
Compliant.

**C-9 — JVM and Ruby are documented as zero-dependency/system-only, and the
explicitly-unsupported categories (container builds, toolchains without an
offline/locked mode, toolchains requiring ambient home/global mutable state,
and GPU/accelerator toolchains) are documented as explicitly unsupported.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("JVM and Ruby remain zero-dependency/
system-only. Container-based builds, toolchains without an offline/locked mode,
toolchains requiring ambient home/global mutable state, and GPU/accelerator
toolchains MUST be documented as explicitly unsupported."). Observed: the
component README documents JVM/Ruby as zero-dependency only, and its
`Explicitly unsupported` section lists container-based builds, toolchains with
no offline/locked mode, toolchains requiring ambient home/global mutable state,
and GPU/accelerator toolchains; the contract states "JVM and Ruby support
zero-dependency projects only" and the same four unsupported categories.
Compliant.

**C-10 — Supported languages and their limitations are documented in the
component README and the component contract.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("Supported languages and their
limitations MUST be documented in the component README and this component's
contract."). Observed: the README carries the per-language support table
(Rust, C/C++, Go, Python, JavaScript/TypeScript, JVM/Ruby, Shell), the
`Intended support order and evidence`, the `Declared limitations` (Rust, Go,
C/C++, host system packages), and the `Explicitly unsupported` section; the
contract carries the `Behavioral guarantees` language-support paragraph and the
per-language native-strategy sections. Both artifacts document the supported
languages and their limitations. Compliant for the documentation obligation
itself (the content of the JavaScript/yarn entry is the subject of F-1).

**C-11 — Rust authoring exposes the validated installed inventory read-only,
initializes sandbox-private selection settings, preserves the project pin or
host default, supports invocation-local `rustup default`, defaults Cargo to
offline/locked, and an ordinary non-ignored native test verifies
inventory/default parity, selection, offline builds, and host non-mutation —
with missing prerequisites failing explicitly rather than skipping or
installing.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` (the Rust-authoring paragraphs and
"An ordinary, non-ignored native test MUST verify installed inventory/default
parity, selection, offline builds and host non-mutation; missing prerequisites
MUST fail explicitly, not skip or install anything."). Observed:
`sandbox_toolchain.rs::rust_authoring_has_host_inventory_private_defaults_and_
offline_builds` snapshots the host rustup inventory before and after an
in-sandbox offline build (host non-mutation + inventory parity) and drives an
offline build; `rust_authoring_builds_workspace_members_with_readonly_provider_
context` exercises declared workspace/path-dependency read-only build resources
and asserts the provider source and lock are unchanged and not writable;
`generic_authoring_preserves_environment_without_rustup_state` covers the
non-Rust authoring environment. The Rust authoring tests use `.expect(…)`
("build galla outside the worktree", "install Bubblewrap on the host", "run
inside a worktree") for their prerequisites, i.e., they fail the test rather
than self-skip or install, matching "missing prerequisites MUST fail
explicitly, not skip or install anything." The Rust verification offline/locked
behavior is separately evidenced by
`offline_cargo_verification_builds_tested_project_denied_network`. Compliant.

**C-12 — Non-Rust authoring retains its approved environment and does not
require Rust; protected component artifacts and peers do not gain write
authority.**
`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` ("Non-Rust authoring MUST retain its
approved environment and MUST NOT require Rust. Protected component artifacts
and peers MUST NOT gain write authority."). Observed:
`sandbox_toolchain.rs::generic_authoring_preserves_environment_without_rustup_
state` is the non-Rust authoring test and does not require Rust; the Rust
authoring tests assert the provider's source, lock, and component artifacts
are read-only and unchanged (no peer write authority). Compliant.

## Discrepancies retained for human arbitration

**F-1 — JavaScript is claimed as vendored-supported for yarn (`yarn.lock`) in
four intent artifacts, but the e2e suite provides no yarn end-to-end test.**
Classification: **mismatched**. Severity: **high**. Owner: **project
architect** / component owner.

Intended text (the four consistent claims):
- `REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` "Current implementation status"
  (line 774): "JavaScript is vendored-supported for npm (`package-lock.json`),
  yarn (`yarn.lock`), and pnpm (`pnpm-lock.yaml`)"; the same note's named
  e2e list contains only
  `javascript_offline_verification_builds_and_tests_denied_network` and
  `javascript_pnpm_offline_verification_builds_and_tests_denied_network` — no
  yarn test name.
- `CONTRACT.md` `vendor` sentence (line 846): "the JavaScript package cache via
  `npm ci`/`yarn install --frozen-lockfile`"; and `Non-Rust offline cache
  preparation` (line 92): the JavaScript lock/mount identity precedence is
  "`pnpm-lock.yaml`, then `yarn.lock`, then `package-lock.json`".
- `README.md` (line 695): the JavaScript/TypeScript row says "Reuse project-
  native npm/yarn cache"; and (line 707) "`kvist vendor` dispatches per
  detected strategy (Go, JavaScript for npm/yarn/pnpm, Python, C/Conan and
  vcpkg); per-language end-to-end evidence is tracked in `maerg/TODOS.yaml`
  and gates the vendored-supported claim."
- `DESIGN.md` ADR-0013 (line 1058): "JavaScript is evidenced for npm/yarn and
  for pnpm".

Observed: `language_offline_e2e.rs` contains exactly two JavaScript tests — the
npm test (a zero-dependency `package-lock.json` project) and the pnpm test (a
`pnpm-lock.yaml` project with a transitive dependency). A search of the
component's `tests/` for a yarn e2e (and of the e2e file's test list) returns
none: there is no test that provisions a `yarn.lock` project on the host and
runs `node --test` offline in the sandbox asserting a successful run. The
requirement's own rule is that "a language is supported for a phase only when an
end-to-end integration test … runs its real build/test offline inside the
Bubblewrap sandbox, asserting a successful run." For yarn, no such test exists,
so the yarn sub-variant does not satisfy the evidence gate the same requirement
imposes, even though the requirement's status note, the contract, the README,
and the design all state yarn is vendored-supported/evidenced.

Analysis: this is an intent-to-evidence mismatch, not a behavior contradiction:
the shared JavaScript topology (one canonical `node --test` command, one
vendored-cache mount, the lock-file digest identity) is the same for every
package manager, and the `yarn install --frozen-lockfile` provisioning and
`yarn.lock` mount-identity precedence are described in the contract and design.
The gap is that the *required* per-variant e2e evidence for yarn is absent, so
the documented "vendored-supported for yarn" claim is not independently
confirmed. Retained so the architect decides between (a) adding the yarn e2e
test to close the gap, or (b) amending the four intent artifacts to scope the
JavaScript claim to the two evidenced package managers (npm, pnpm) and tracking
yarn separately. No behavior change is implied; either resolution is a
test-evidence or intent-documentation change.

**F-2 — ADR-0013 describes the Go offline strategy as a committed `vendor/`
directory with `go test -mod=vendor`, which contradicts the requirement, the
contract, the design, and the e2e evidence (all of which use the native module
cache with `go test -mod=readonly` and assert no `vendor/` is created).**
Classification: **mismatched** (a referenced design record is stale relative to
the governing requirement/contract/implementation). Severity: **medium**.
Owner: **project architect**.

Intended text (ADR-0013, the design record the requirement and contract point
to): "Go: `go test -mod=vendor ./...` with `GOPROXY=off`, `GOTOOLCHAIN=local`,
`GOFLAGS=-mod=vendor`"; "Go end-to-end evidence … vendors a real small Go
module on the host … and runs `go test -mod=vendor ./…` network-denied"; "Go —
none: the vendored material is the committed `vendor/` directory, which travels
inside the component mount; `go test -mod=vendor` builds entirely from it and
needs no resolver configuration."

Observed: `REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT` states "Go uses the native
read-only module cache"; `CONTRACT.md` `Native Go module cache` states the
verification command is "`go test -mod=readonly ./…`" and "It does not create
or refresh project `vendor/`"; `DESIGN.md` ADR-0013 (the component's own
design) describes Go as "`kvist vendor` runs native `go mod download`; the
selected module cache is mounted read-only at `/workspace/go-modules`, without
a source copy. The profile runs `go test -mod=readonly ./…`"; and the e2e test
`go_offline_verification_builds_and_tests_denied_network` provisions through a
native host `GOMODCACHE` and asserts `!root.join("vendor").exists()` ("native
preparation must not copy module sources").

Analysis: the implementation and the in-scope e2e match the requirement, the
contract, and the component's `DESIGN.md`; only the ADR-0013 document still
carries the earlier committed-`vendor/`/`-mod=vendor` description of the Go
strategy. This is an intent-artifact (design-record) inconsistency with no
observed behavioral risk, because the requirement/contract/design/e2e are
mutually consistent. Retained so the architect decides whether ADR-0013's Go
section should be corrected to the native-module-cache/`-mod=readonly`
description (or a reasoned note added explaining the evolution). This is a
design-record correction only; no behavioral change is implied.

**F-3 — `DESIGN.md`'s evidence-status sentences are stale and internally
inconsistent with the e2e evidence that is present.**
Classification: **mismatched** (design record stale / self-contradictory).
Severity: **low**. Owner: **component owner** / project architect.

Intended text (`DESIGN.md` ADR-0013, lines 1057–1058, immediately after the
per-language profiles): "Go, JavaScript, and Python are claimed with that
evidence; C/C++ is tracked in `TODOS.yaml`. JavaScript is evidenced for npm/
yarn and for pnpm; C/C++ is evidenced for Conan and for vcpkg."

Observed: both C/C++ e2e tests (`c_offline_verification_builds_and_tests_denied_
network`, `c_vcpkg_offline_verification_builds_and_tests_denied_network`) are
present in `language_offline_e2e.rs` and self-skip on hosts without the live
sandbox or the C toolchain. The same paragraph therefore both (a) says C/C++
"is tracked in `TODOS.yaml`" (i.e., not yet evidenced) and (b) says "C/C++ is
evidenced for Conan and for vcpkg"; and it says "JavaScript is evidenced for
npm/yarn" while listing only the npm and pnpm e2e tests (see F-1 for the yarn
gap). The component README, by contrast, says C/C++ "validated end-to-end
(`language_offline_e2e`)" and "it is claimed as vendored-supported," which is
consistent with the e2e suite.

Analysis: the e2e suite (the authoritative in-scope evidence) shows both C/C++
tests exist and are passing-shaped, so the "C/C++ is tracked in `TODOS.yaml`"
sentence is stale, and the paragraph is internally inconsistent with itself.
This is a documentation-staleness defect, not a behavioral one. Retained so the
owner updates the `DESIGN.md` status sentences to match the e2e evidence (and
the yarn scope, per F-1). No behavior change is implied.

**F-4 — The sandbox-language-tooling PATH clause (bin-only search paths
including `/usr/local/bin`, excluding the standard administrative directories
before and after canonicalization) has no in-scope executable evidence within
`maerg/tests/`, and the implementation record does not restate the per-language
offline-verification behavior for a source-blind reviewer.**
Classification: **underspecified** (in-scope executable evidence absent; no
contradictory in-scope observation). Severity: **low**. Owner: **project
architect**.

Intended text (`REQUIREMENTS.md` `REQ-LANGUAGE-SUPPORT`, first paragraph):
"Sandbox language tooling MUST use bin-only search paths, including
`/usr/local/bin`, and exclude the standard administrative directories before
and after canonicalization when locating host tools."

Observed: no in-scope test in `maerg/tests/` asserts the bin-only search-path /
administrative-directory-exclusion behavior when locating host tools (the
in-scope e2e tests exercise the language build/test offline path, not the
tool-location policy; the language e2e helper locates the runner/backend via
environment overrides and fixed candidate paths). Separately, `maerg/IMPL.md`
was checked as the implementation-observed-behavior source for this topology and
contains no section describing the per-language offline verification behavior
(the shared topology, the per-language profiles, the vendored mounts, or the
canonical/approved commands); the only language-verification-adjacent
implementation-record content is a one-line dispatch reference in a prior
security-finding closure. The clause is consistent with the design/contract
description of the fixed sandbox `PATH` and the bound system prefixes, but it
is not independently evidenced by an in-scope executable test, and the
implementation record does not restate it for anchoring.

Analysis: this is an evidence-gap for a source-blind reviewer rather than a
behavioral contradiction (nothing observed contradicts the clause). Retained so
the architect decides whether (a) an in-scope test asserting the tool-location
policy should be added, and/or (b) a clean-slate implementation-observation
section for the per-language offline verification topology should be added to
`IMPL.md` so a later source-blind review can anchor to the record rather than to
intent documents and e2e assertions alone. No behavior change is implied.

## Disposition and retention

This record classifies the `REQ-LANGUAGE-SUPPORT` clauses and retains F-1, F-2,
F-3, and F-4 for explicit human arbitration. Per `ROOT_CONTRACT.md` and the
runbook, no requirement, contract, design, or `IMPL.md` artifact was altered to
erase any mismatch, and no recommendation in this record has been applied.
Recommended arbitration (not applied):
- **F-1** — either add a yarn (`yarn.lock`) end-to-end test to
  `language_offline_e2e.rs`, or amend the requirement status note, the
  contract, the README, and the design to scope the JavaScript vendored-
  supported claim to the two evidenced package managers (npm, pnpm) and track
  yarn separately.
- **F-2** — correct ADR-0013's Go section to the native-module-cache/
  `go test -mod=readonly` strategy (or record a reasoned note of the evolution).
- **F-3** — update the stale/self-contradictory `DESIGN.md` evidence-status
  sentences to match the e2e evidence.
- **F-4** — add an in-scope test for the tool-location policy and/or an
  implementation-observation section in `IMPL.md` for this topology.

This record is a compliance-review observation, not a review receipt, not a
compliance certification, and not human acceptance. The presence of this report
does not itself accept the per-language offline verification work.
