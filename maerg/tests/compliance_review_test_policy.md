<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Real Test-Policy Approval and Verification (`maerg`)

**Component:** `maerg`
**Task:** `test-policy-verification-compliance-review`
**Scope:** Source-derived policy approval and verification behavior for the real
per-component `[test_policy]` command set, compared against the approved policy,
the component contract, and the design.
**Reviewer stance:** Source-blind, independent compliance reviewer. The
implementer and the security auditor did not author this record; this reviewer
did not read implementation source, manifests, or Git history as evidence.

## Review boundary and independence

This is a source-blind comparison between intended behavior (`REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`, `ROOT_CONTRACT.md`) and observed behavior
(`IMPL.md` plus bounded test/execution evidence). It is the fourth and final
stage of the `test-policy-verification` chain (tests → implementation →
security audit → compliance review). Per the runbook, a compliance reviewer may
read only the target intent set, the current `IMPL.md`, bounded test evidence,
the nearest ancestor contract, and `ROOT_CONTRACT.md`.

**Read (permitted):** `maerg/REQUIREMENTS.md`, `maerg/CONTRACT.md`,
`maerg/DESIGN.md`, `maerg/IMPL.md`, `ROOT_CONTRACT.md`, `REVIEW_RUNBOOK.md`
(the review procedure), `maerg/tests/test_policy.rs` (bounded test evidence),
and the machine-local chain attempt log
`.kvist-attempts/test-policy-verification-security-audit.jsonl` (bounded
execution evidence for the immediately preceding security-audit stage).

**Read as bounded *policy* evidence (the approved-policy carrier, not
implementation source):** the working-tree project config `kvist.toml`
`[test_policy]` section and the tracked `kvist.toml.example`. Reading the
approved policy is the explicit object of this task.

**Not read:** `maerg/src/**`, `Cargo.toml` / `Cargo.lock`, Git history or
diffs, prior compliance conclusions, and peer or parent implementation
artifacts. The reviewer executed no tests, builds, or linters.

## Bounded evidence relied upon

- **Approved policy (observed, working-tree `kvist.toml`, gitignored):**
  `sha256:21a7d93b6650d00cf21871e4f975b4bc764e7caecf04127ea63c92cda5e5256b`
  - `[test_policy]` = 4 real commands
    (`maerg`/`sav`/`galla`/`skott` → `cargo test`),
    `timeout_seconds = 1800`, `max_output_bytes = 1048576`,
    `environment_allowlist = [PATH, RUST_BACKTRACE, CARGO_HOME, RUSTC, TERM]`,
    `working_directory = "component"`. No `/usr/bin/echo` placeholder.
- **Reproducible policy (tracked `kvist.toml.example`):**
  `sha256:db3611386831deeb34374fed0309e2964d77baf4bfd7f4f2e068ad3089a05ad1`
  - Same four real `cargo test` commands and identical bounds; comment: "They
    must be real test executions … never placeholders: recorded verification
    success is compliance evidence."
- **Bounded test evidence (tracked `maerg/tests/test_policy.rs`):**
  `sha256:975e2fde675c8b770539b7749ef846fe4f2ccf2f01ca4ee02be4ff8d345380cd`
  - `project_test_policy_has_no_placeholder_commands`
  - `project_test_policy_digest_covers_command_set`
  - `policy_digest_is_deterministic_and_bound_to_the_full_policy`
  - `policy_digest_fails_closed_when_serialization_is_unavailable`
- **Chain execution evidence (security-audit attempt log):** the recorded
  `policy_identity` for the chain was
  `sha256:6f98bfede68bcd48a9da3d3b49b34e117694378f3728cf49547c1614a7b33543`
  and `runner_identity`
  `sha256:7d0ddf564547997115422dde4dd752a58f3acae97c0a69a92c780c62addea8f6`.
  The audit recorded the fail-closed `compute_policy_hash → Result<String>`
  fix, the zero-limit rejection fix, and the per-component environment-allowlist
  validation fix, and noted it could not run `cargo test` in its own context.

This record is *not* execution proof. The `test_policy` suite is run
automatically by Kvist in the verification sandbox after this turn; no pass/fail
count from that run is asserted here.

## Method

Each approval and verification obligation relevant to the real test policy was
drawn from `REQUIREMENTS.md` (`REQ-EXECUTION-BOUNDARY`,
`REQ-SUPERVISED-EXECUTION`, `REQ-AGENT-READ-SCOPE`/`REQ-COMPONENT-WRITING-SCOPE`,
and the offline-locked verification rules), `CONTRACT.md`
(`Security and authority`, `Compatibility and verification`, the
`task approve-policy` surface, and the per-component `[test_policy]` execution
path), `DESIGN.md` (`Planned dogfooding execution boundary`, `Planned review
evidence and acceptance state`, `Verification strategy`), and
`ROOT_CONTRACT.md` (`Change and compliance rules`). Each was classified
**compliant**, **mismatched**, **approved-deferred**, or **underspecified**,
with component path, locator, intended text, observed behavior/evidence,
severity, and owner.

## Satisfied clauses (compliant)

**C-1 — Placeholder verification is rejected; the approved policy is real.**
`CONTRACT.md` `Compatibility and verification`; `REQUIREMENTS.md`
`REQ-EXECUTION-BOUNDARY`; `kvist.toml.example` comment. The observed policy
contains four real `cargo test` commands and no `echo` placeholder, so on a
host that carries the policy the guard's assertions (no `echo` program, no
`'Verification passed'` banner, no empty command) hold against it. Its
documented purpose ("this test fails the suite until real per-component
verification commands are committed") is therefore met *where the policy is
present*; the clean-checkout vacuity it leaves is recorded separately as F-1.

**C-2 — Policy approval digests the actual command set (digest contract).**
`REQUIREMENTS.md` `REQ-SUPERVISED-EXECUTION` (unique reviewable attempt);
`CONTRACT.md` `Security and authority` (approval bound to exact policy bytes);
`DESIGN.md` `Planned dogfooding execution boundary` (policy identity is the
authenticated execution-approval digest, "not a locally computed unapproved
hash"). `IMPL.md` (agent/execution-approval observation): "Execution approval
also incorporates … test policy. Task execution passes its approved runner,
capability probe, and policy digest into effect execution." The bounded tests
`policy_digest_is_deterministic_and_bound_to_the_full_policy` and
`project_test_policy_digest_covers_command_set` assert determinism, a
`sha256:` form, and sensitivity to command, timeout, output-cap, and
environment-allowlist changes.

**C-3 — Approval is fail-closed (no constant-digest collapse).**
`CONTRACT.md` `Security and authority` (repository files, configuration, and
subprocess output are untrusted). The audit's fail-open defect
(`serde_json::to_string(policy).unwrap_or_default()` hashing the empty string)
is recorded as fixed to `Result<String>`; `IMPL.md` "Source-derived approval
and effect-timeout addendum" confirms "Approval loading authenticates the
recorded material; execution compares it with current material."
`policy_digest_fails_closed_when_serialization_is_unavailable` asserts a real
policy never hashes to the empty-string digest.

**C-4 — Verification command is bound to the approved digest, shell-free.**
`CONTRACT.md` `Security and authority` ("Commands are split and spawned
directly by Kvist without shell expansion"); `DESIGN.md` `Planned dogfooding
execution boundary` (argv[0] resolved to an exact regular non-symlink
executable, canonicalized and content-hashed; bounded argv/environment limits).
Security-audit evidence: `find_test_command` returns the approved command
string → split into argv → executed directly (no shell); `resolve_program`
requires a regular non-symlink executable resolved via the request `PATH`.

**C-5 — Environment, output, and timeout bounds are enforced and
digest-bound.** `CONTRACT.md` `Compatibility and verification` / `Security and
authority`; `DESIGN.md` `Planned dogfooding execution boundary`. Security-audit
evidence: `allowed_environment` returns only the intersection of the
`[sandbox]` and `[test_policy]` allowlists (and the per-component allowlist now
passes the same portable-name + 256-entry validation); output cap and timeout
are each enforced twice (host-side bounded drain/deadline and the galla runner
`max_output_bytes` / `wall_time_ms` with process-group SIGKILL on breach). The
digest binds `timeout_seconds` and `max_output_bytes` (C-2), so a changed bound
invalidates the approval.

**C-6 — `task approve-policy` is the approval surface and chains to
execution.** `CONTRACT.md` command surface: `task approve-policy [PROJECT_DIR]`
is listed, and "gating success messages chain … `task approve-policy` points
at `task run`." The implementation stage re-ran `approve-policy` so the
recorded digest covers the actual (real) command set, as required by the
`test-policy-verification-implement-code` intent.

**C-7 — No agent write authority over approval material or canonical
evidence.** `REQUIREMENTS.md` `REQ-COMPONENT-WRITING-SCOPE` (authoring agents
never write `.kvist` state or evidence) and `ROOT_CONTRACT.md` (agents must not
write approval state or canonical evidence). The chain attempt log records the
approved write scope as exactly `maerg/src` and `maerg/tests`; the reviewer's
scope here is the same. Approval records and policy state are outside that
scope.

## Discrepancies (mismatched / underspecified — retained for arbitration)

**F-1 (HIGH, mismatched) — The placeholder guard fails open on a clean
checkout, so it cannot enforce its stated "committed real commands"
expectation.**
- *Locator:* `maerg/tests/test_policy.rs::project_test_policy_has_no_placeholder_commands`
  (early `return` when `kvist.toml` is absent) vs.
  `test-policy-verification-write-tests` intent ("Tests fail while the committed
  policy contains placeholder echo commands and pass once real per-component
  verification commands are committed") and `kvist.toml.example`
  ("recorded verification success is compliance evidence").
- *Observed:* The guard's own doc-comment and code state that `kvist.toml` is
  "project-local (not committed)" and that "when it is absent there is no
  policy to inspect and the test passes." The `.gitignore` confirms
  `kvist.toml` is ignored. Therefore on a fresh checkout that carries no local
  `kvist.toml`, the guard — and by extension the placeholder detection the
  chain was built to provide — passes vacuously while zero per-component
  verification commands exist. The "committed" half of the intended outcome is
  not reachable because the policy is by design not commitable.
- *Why it matters:* The chain's purpose is to "make placeholder verification
  detectable and replaceable before the dogfood flow relies on recorded
  verification evidence." A guard that cannot detect placeholders where the
  policy is absent (i.e., on any clean checkout) does not establish that
  detectability in the places the evidence must be reproducible.
- *Arbitration options:* (a) make the test fail-closed on an absent policy
  rather than passing; (b) assert against a tracked, reproducible canonical
  policy (e.g., a committed policy file or the example) so the guard has a
  committed subject; (c) record a reasoned exception that the policy is
  intentionally machine-local and that clean-checkout placeholder detection is
  therefore out of scope.

**F-2 (HIGH, mismatched) — The approval record and the approved command set are
not retained in version control, so "independent evidence that recorded
verification matches the approved policy" is host-bound, not reproducible.**
- *Locator:* task purpose "Preserve independent evidence that recorded
  verification matches the approved policy"; `ROOT_CONTRACT.md` ("Record
  intent-to-implementation discrepancies for explicit arbitration"; keep
  durable state in version-controlled project files); `CONTRACT.md`
  `Compatibility and verification` ("`IMPL.md` must be derived independently…
  Compliance requires a separate source-blind comparison").
- *Observed:* The only approved-policy carrier (`kvist.toml`) and the only
  approval-state location (the component/`$XDG` state directories, `.kvist`,
  and `.kvist-attempts`) are gitignored. The working-tree inspection shows no
  tracked file holds the recorded execution-approval digest, the runner
  identity, or the approved command set; the tracked `kvist.toml.example`
  carries commands and bounds but no recorded approval digest. The chain's own
  digest tests (C-2) verify the *hash function's* sensitivity, but no tracked
  artifact records the *specific* approval that bound the recorded verification
  run, and the approval state that would carry it is untracked.
- *Why it matters:* A clean checkout can recompute the digest (C-2) but cannot
  re-derive which policy was *approved*, on which runner, or that the recorded
  verification actually matched it. The "independent, reproducible" evidence
  this review is meant to preserve is therefore not preserved in the only
  durable, shareable medium the project uses for compliance (version control).
- *Arbitration options:* (a) retain a version-controlled, digest-bound approval
  receipt (policy bytes + `policy_identity` + `runner_identity`) in VCS, or a
  tracked test that pins the recorded digest to the tracked policy; (b)
  explicitly scope the approval evidence as machine-local and record a reasoned
  exception stating that current recorded verification is not independently
  reproducible from a clean checkout; (c) defer to a later dogfood-readiness
  task with an explicit deferral.

**F-3 (MEDIUM, underspecified) — The per-component `[test_policy]` Rust command
is not pinned to the contract's exact offline-locked verification command.**
- *Locator:* `REQUIREMENTS.md` offline-verification rule "Verification MUST run
  exactly `cargo test --offline --locked`, with denied network …" and "Builds
  and verification MUST NEVER install, upgrade, or modify a toolchain";
  `CONTRACT.md` per-component `[test_policy]` path ("the per-component
  `[test_policy]` command runs in the network-denied sandbox … with no vendored
  mounts and no `$HOME`"); observed policy `command = "cargo test"`.
- *Observed:* The approved Rust per-component command is bare `cargo test`,
  without `--offline --locked`. The network-denied sandbox prevents live
  downloads, and the policy's environment allowlist and offline runner topology
  are the enforcement layer; however, the *specific* offline-locked invocation
  the contract names for Rust verification is not what the per-component policy
  command carries. For this workspace the offline-locked guarantee for the
  `[test_policy]` path rests on the sandbox denial plus the operator-chosen
  command string, not on the command itself.
- *Arbitration options:* (a) pin the per-component command to
  `cargo test --offline --locked` (and/or add a test that rejects a
  network-relying command); (b) clarify in the contract/design that a
  per-component `[test_policy]` command must itself be a network-safe
  invocation and is covered by the sandbox denial; (c) record an exception
  documenting the intended fail-closed posture for the generic path.

**F-4 (LOW, underspecified) — No bounded end-to-end evidence in this chain that
the configured per-component command set is *supervised-executed*
per-component.**
- *Locator:* `test-policy-verification-write-tests` intent ("supervised
  execution runs the configured per-component command set"); `DESIGN.md`
  `Verification strategy` (process supervision, timeout, sandbox requests use
  integration tests); `REQ-SUPERVISED-EXECUTION`.
- *Observed:* The four `test_policy` tests exercise the digest contract and
  placeholder rejection (unit/integration level against the config API). They
  do not drive a real `task run` that supervises each component's command in
  the network-denied sandbox, nor do they assert the per-component
  dispatch/supervision path. The generic execution path is covered elsewhere in
  the suite (dogfood/execution boundaries), but the test-policy chain itself
  does not carry that end-to-end evidence.
- *Arbitration options:* (a) add a bounded end-to-end fixture that runs a
  recorded per-component verification under the approved policy and asserts
  supervised execution; (b) designate this as an approved-deferred gap owned by
  the supervised-execution coverage; (c) document where the generic-path
  supervision coverage lives so the traceability is explicit.

**F-5 (LOW, transient) — The chain's fixes are source-complete but uncommitted
at review time.**
- *Locator:* `test-policy-verification-security-audit` (defects: fail-open
  digest, zero-limit acceptance, per-component allowlist validation); working
  tree.
- *Observed:* The working tree shows the chain's implementation files
  (`maerg/src/config.rs`, `maerg/src/task_commands.rs`,
  `maerg/tests/test_policy.rs`) as modified-but-uncommitted relative to
  `HEAD`. The security-audit log states the fixes are "source-complete" and
  that `cargo test` was not run in that context. The compliant digest behavior
  described in C-2/C-3 is therefore present in the working tree but not yet in
  the version-controlled state.
- *Arbitration options:* none required beyond the normal flow — the
  verification sandbox runs the suite and the implementation/finalization step
  commits the accepted change. Recorded here so the "current" state an
  arbitrator inspects is not mistaken for the "committed" state.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `test_policy` suite is run automatically by Kvist in the
  verification sandbox after this turn, and no result from that run is
  asserted.
- The observed policy bytes (`kvist.toml`) are from the live working tree and
  are gitignored; they are the best available statement of the approved policy
  but are not themselves version-controlled (see F-2).
- The security-audit evidence is the immediately preceding stage's attempt
  log, used as bounded corroboration of mechanism, not as certification.
- This review does not read `maerg/src/**`, manifests, or Git history; claims
  about enforcement internals (e.g., exact SIGKILL behavior, the
  `allowed_environment` intersection) are attributed to `IMPL.md` and the
  security-audit evidence, not to direct source inspection.

## Arbitration required

The project architect owns arbitration. This review does **not** modify
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or `IMPL.md` to erase any
mismatch, and it does not treat external process success as acceptance. The
required decisions, in priority order, are:

1. **F-1 (HIGH):** Decide the placeholder-guard's clean-checkout posture —
   fail-closed on an absent policy, assert against a tracked canonical policy,
   or record a reasoned exception that machine-local policy makes
   clean-checkout detection out of scope.
2. **F-2 (HIGH):** Decide how "independent evidence that recorded verification
   matches the approved policy" is preserved in a reproducible, version-
   controlled form — a tracked digest-bound approval receipt / pinned test, an
   explicit exception that the evidence is machine-local, or an approved
   deferral.
3. **F-3 (MEDIUM):** Decide whether the per-component Rust `[test_policy]`
   command must be pinned to `cargo test --offline --locked` (or a
   network-safe invocation), or clarify/except the sandbox-denial posture.
4. **F-4 (LOW):** Decide whether to commission a bounded end-to-end
   supervised-execution fixture for the configured command set, or designate
   it an approved-deferred gap with explicit owner.
5. **F-5 (LOW):** No action beyond the normal verification + commit flow;
   recorded for state clarity only.

This record, the fresh `IMPL.md`, and any approved arbitration decision are to
be retained in version control. No raw provider transcript, credential, secret,
or temporary review workspace is retained.
