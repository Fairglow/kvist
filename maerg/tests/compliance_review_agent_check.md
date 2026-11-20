<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: `agent check` and `agent role list` Behavior (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `agent-check-compliance-review`
**Scope:** Source-derived `agent check` and `agent role list` behavior — the new
interactive command surface and the role-table presentation change — compared
against the agent-integration requirement (`REQ-AGENT-INTEGRATION`) and the
provided-interfaces contract.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is the
final stage of the `agent-check` chain (write-tests → implement-code →
security-audit → compliance review). The implementer and the security auditor
did not author this record; this reviewer did not read implementation source,
dependency manifests, or Git history as evidence.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read
only the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the
`IMPL.md` and bounded test evidence; the nearest ancestor component
`CONTRACT.md` (none — `maerg` is the root component); and `ROOT_CONTRACT.md`.
Each clause is classified **compliant**, **mismatched**, **approved-deferred**,
or **underspecified**, with component path, stable locator, intended text,
observed behavior/evidence, severity, and owner.

**Read (permitted):** `maerg/REQUIREMENTS.md`
(`sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e`),
`maerg/CONTRACT.md`
(`sha256:957ce69d562103d49f26cac15691f96f1fe3ab9ab0e43962af6cb89776d9d53a`),
`maerg/DESIGN.md`
(`sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f`),
`maerg/IMPL.md`
(`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`),
`ROOT_CONTRACT.md`
(`sha256:b4dd022afb6237cd20c255a0b9476348e9e76e43bfae4878d4626136be535252`),
`REVIEW_RUNBOOK.md` (procedure), and bounded test evidence:
`maerg/tests/agent_check.rs`
(`sha256:d8efaf07347438429692c2e157d3653e5623da0816d069ea909398e12f020064`),
`maerg/tests/wizard.rs`
(`sha256:10b5e6d072449e2c35c0c79f5d5fd3085a11ee158cea0db1d17c621043c520c0`),
and `maerg/tests/agent.rs`
(`sha256:9f159d51777c3dd384be34d498b274c5489f83381633feb4a9b5552351700503`).
The task chain definitions in `maerg/TODOS.yaml` were consulted for the
`agent-check` task `id`/`title`/`description`/`context`/`status` fields only, to
scope the chain's claimed surface; they were not used as evidence of observed
behavior.

**Not read:** `maerg/src/**` implementation source (including `src/wizard.rs`,
`src/cli.rs`), `sav/`, `galla/`, `skott/` source, `Cargo.toml`/`Cargo.lock` as
evidence, Git history or diffs, prior compliance conclusions, and peer or parent
implementation artifacts. No tests, builds, linters, or `cargo` invocations
were executed in this review; the `agent_check`, `wizard`, and `agent` suites
are run automatically by Kvist in the verification sandbox after this turn, and
no pass/fail count from that run is asserted here.

## Contract revision note (state clarity)

The `CONTRACT.md` digest read here (`sha256:957ce69d…`) differs from the
revision recorded by the immediately preceding `agent-config-surface`
compliance review (`sha256:646ba4be…`), while `REQUIREMENTS.md`, `DESIGN.md`,
`IMPL.md`, and `tests/agent_check.rs` carry digests identical to that record.
The bounded, source-blind reading is that the provided-interfaces contract was
revised after the prior review and now declares the `agent check` interactive
surface and the `agent role list` role-only presentation (the "new interactive
command surface and a presentation change" named in this task's context). This
review compares behavior against the **current** contract text. This note is
state observation only; no Git history was inspected.

## Method

Every `agent check` and `agent role list` obligation was drawn from
`REQUIREMENTS.md` (`REQ-AGENT-INTEGRATION`, third and fourth paragraphs) and
`CONTRACT.md` (`Provided interfaces`: the `agent check [--global]` line and the
interactive-exceptions paragraph), compared with the observed behavior recorded
in `IMPL.md` and the bounded test evidence. The test assertions are the
executable statement of observed behavior in the places `IMPL.md` is silent.
The fixed prompt for the check and the setup qualification is a single shared
constant in the bounded evidence (`sav::SETUP_TEST_PROMPT`, asserted to equal
the fixed prompt by `wizard.rs`
`wizard_qualifies_external_argv_with_fixed_prompt_and_discloses_authority`), so
the setup-side pinning of that constant is treated as shared, not as
check-specific, evidence.

## Satisfied clauses (compliant)

**C-1 — `agent role list` presents only the predefined roles and their assigned
model profiles, and never presents model profile names as roles.**
`REQ-AGENT-INTEGRATION` ("`agent role list` MUST present only the predefined
roles and their assigned model profiles and MUST NOT present model profile
names as roles"); `CONTRACT.md` `Provided interfaces` (predefined roles
developer, architect, security-reviewer). Evidence:
`agent_role_list_presents_only_predefined_roles` — with two configured model
profiles (`qwen-coder`, `llama-local`) and one role binding, every row of the
role table names only one of the three predefined roles (`architect` |
`developer` | `security-reviewer`), while the assigned profile and the available
profile remain visible. Compliant.

**C-2 — `agent check` requires an explicit interactive acknowledgement before
executing, and that acknowledgement is itself a cancellation point: refusal
executes no test command and changes nothing.** `REQ-AGENT-INTEGRATION`
("MUST NOT execute any test command without an explicit interactive
acknowledgement, and that acknowledgement MUST itself be a cancellation point");
`CONTRACT.md` `Provided interfaces` ("obtains an explicit interactive
acknowledgement before executing any profile command; that acknowledgement is
also a cancellation point"). Evidence: `agent_check_refusal_executes_nothing_\
and_changes_nothing` — with a marker script profile, answering `n` reports
`cancelled`, the marker output file is never created (no test command executed)
and the configuration is byte-identical; `agent_check_cancels_at_the_failure_\
prompt_without_changes` — answering `q` at the failure prompt reports
`cancelled` with configuration unchanged; `cancelled_binding_and_declined_\
check_have_no_effects` in `wizard.rs` — a declined `check_agents` run leaves the
configuration byte-identical. Compliant.

**C-3 — Each profile is live-verified on the host with the runtime's bounded
verification and the fixed minimal prompt; healthy profiles pass without
changing configuration.** `REQ-AGENT-INTEGRATION` ("live-verify each model
profile configured in the selected scope by executing that profile's exact
configured command on the host with the fixed minimal prompt `Reply with
exactly: OK`"); `CONTRACT.md` `Provided interfaces`. Evidence:
`agent_check_reports_healthy_profiles_without_changes` — two `echo '{prompt}'`
profiles pass after the `y` acknowledgement, both are named in the output, an
`ok` result is reported, and the configuration is byte-identical. The exact
prompt constant is pinned on the setup side (C-6); see F-2 for the check-side
assertion gap. Compliant.

**C-4 — For each failing profile, the check offers removal (the standard
profile removal semantics) or temporary ignore (configuration unchanged).**
`REQ-AGENT-INTEGRATION` ("MUST offer profile removal (using the standard profile
removal semantics) or temporary ignore (which leaves the configuration
unchanged)"); `CONTRACT.md` `Provided interfaces` ("offers removal (the standard
profile removal semantics, including role-binding clearance) or ignore for now
(configuration unchanged)"). Evidence: `agent_check_removes_a_failed_profile_\
on_choice` — a `/usr/bin/false` profile, removed by choice `1`, no longer
appears in the configuration; `agent_check_ignores_a_failed_profile_on_choice` —
choice `2` reports `ignored` and the configuration is byte-identical. The
*role-binding clearance* facet of removal is not directly asserted in the
bounded evidence; see F-2. Compliant for the offered-removal/ignore behavior as
declared by the requirement; the contract-specific clearance facet is noted.

**C-5 — The check remains cancellable at every prompt.** `REQ-AGENT-INTEGRATION`
("MUST remain cancellable at every prompt"). Evidence: the refusal path (C-2),
the failure-prompt cancellation (`agent_check_cancels_at_the_failure_prompt_\
without_changes`), and the setup-side `SIGINT` cancellation precedent
(`force_cannot_persist_cancelled_qualification`) together pin that each prompt
is a cancellation point with no side effects. Compliant.

**C-6 — `agent check --global` honors the user configuration scope, and scope
profiles without configuration report nothing to check.** `CONTRACT.md`
`Provided interfaces` (the `agent check [--global]` line; "the project-local
`kvist.toml` by default, the user configuration with `--global`"). Evidence:
`agent_check_global_scope_uses_the_user_configuration` — with `KVIST_CONFIG_PATH`
pointing at a user configuration containing `echo-global`, the check verifies
that profile, reports `ok`, and leaves the user configuration byte-identical;
`agent_check_without_profiles_reports_nothing_to_check` — with no configured
profiles the check reports `nothing to check` and succeeds. Compliant.

**C-7 — The acknowledgement gate keeps host execution of configured commands
within the declared authority model.** `ROOT_CONTRACT.md` (non-negotiable
architecture: external commands resolved without a shell and covered by an
explicit human-approved execution policy; "A one-off host command may use an
explicit invocation acknowledgement that states its authority and scope");
`REQ-AGENT-INTEGRATION`. Evidence: the acknowledgement gate is exercised as a
cancellation point in C-2; refusal never reaches command execution
(`agent_check_refusal_executes_nothing_and_changes_nothing`). The security-
audit stage of this chain is completed per the queue, but its report is outside
this reviewer's permitted read set; this review neither relies on nor repeats
its conclusions. Compliant.

**C-8 — In global `--json` mode, standard output carries exactly one result
object, as in the other interactive agent flows (partially pinned).**
`CONTRACT.md` `Provided interfaces` ("With global `--json`, the transcript goes
to standard error and standard output carries exactly one result object, as in
the other interactive agent flows"). Evidence: the interactive-agent-flow
precedent is pinned for setup — `json_agent_setup_keeps_stdout_machine_readable`
(stdout is exactly one result object; qualification output and stderr material
are absent from stdout; the transcript appears on stderr). The `agent check`
`--json` form itself has no bounded test in the tree; see F-2. Partially
compliant: the declared behavior is consistent with the retained flow precedent,
but the check-specific JSON mode is not pinned.

## Discrepancies (retained for human arbitration)

**F-1 (MEDIUM, mismatched) — `IMPL.md` carries no observation of the
`agent check` command or the `agent role list` presentation, so the observed-
behavior carrier is absent for this change.**
- *Locator:* `maerg/IMPL.md`
  (`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`);
  `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md` describes
  observed implementation behavior and is not copied from intended requirements,
  contracts, or designs"; "A clean-slate documenter and a separate compliance
  reviewer must verify implemented behavior before it is declared compliant");
  `REVIEW_RUNBOOK.md` (the compliance reviewer reads "the newly produced
  `IMPL.md`").
- *Intended (declared):* `IMPL.md` states public behavior, guarantees, limits,
  state effects, failure paths, and known uncertainty, derived independently
  from source; the compliance pass compares intent against that fresh
  observation.
- *Observed:* `IMPL.md`'s `Observed public contract → Command and workflow
  surface` names "agent configuration and qualification" among the commands but
  contains no heading, addendum, or scoped observation of the `agent check`
  interactive surface or the role-list presentation. A term sweep of `IMPL.md`
  for `cancellation point`, `nothing to check`, `role-binding`, `deterministic
  order`, `acknowledge` (check context), and `Reply with exactly` found no
  observation of this change; the only qualification-adjacent observations are
  in unrelated sections (boundary fixtures, resilience phases, and the
  non-execution note at "no configured agent command is executed by this path").
- *Why it matters:* the task's purpose is to "preserve independent evidence that
  the agent check matches its declared requirements and contract." With the
  observed-behavior carrier absent, the source-blind comparison for C-1…C-8
  degrades to tests-only, and the runbook's primary carrier expectation is not
  met for this change. The bounded tests cover the core acceptance, refusal,
  failure-prompt, ignore, global-scope, and role-list behaviors, but the
  contract-specific facets noted in F-2 are not pinned by tests *or* by
  `IMPL.md`.
- *Arbitration options:* (a) commission a clean-slate documenter pass over the
  `agent check` / role-list change so `IMPL.md` carries the observed behavior
  (acknowledgement gate, fixed prompt, scope merge, deterministic order,
  removal/ignore, `nothing to check`, JSON mode), then re-run this compliance
  pass against it; (b) record a reasoned exception that the bounded
  `agent_check.rs`/`wizard.rs` suites are the retained observation for this
  chain; (c) approved deferral with an owner. This review does not modify
  `IMPL.md`.

**F-2 (LOW, underspecified in bounded evidence) — Three contract-specific facets
of `agent check` are declared in the contract but not pinned in the bounded
tests, and `IMPL.md` is silent (see F-1).**
- *Locator:* `CONTRACT.md` `Provided interfaces` (interactive-exceptions
  paragraph for `agent check`).
- *Facet (a) — check-side fixed prompt:* the contract fixes the prompt to `Reply
  with exactly: OK`. The bounded `agent_check.rs` tests use `echo '{prompt}'`
  and `/usr/bin/false` profiles and assert on pass/fail and configuration
  effects, but never assert that the check issued that exact prompt. The fixed
  prompt constant is pinned only on the setup side (`wizard.rs` asserts
  `sav::SETUP_TEST_PROMPT` equals the fixed prompt).
- *Facet (b) — exhausted standard input ends the check:* the contract lists
  "refusal, cancellation input, exhausted standard input, or interruption" as
  no-execution end conditions. The bounded tests pin refusal (`n`),
  cancellation (`q`), and (setup-side) interruption, but no `agent check` test
  feeds an exhausted stdin and asserts no execution and no configuration
  change.
- *Facet (c) — JSON-mode single result object:* the contract requires exactly
  one result object on stdout for `agent check --json` (C-8). The retained
  `agent_check.rs` suite contains no `--json` case; the flow precedent is pinned
  for setup only.
- *Facet (d) — role-binding clearance on removal:* the contract says removal
  uses "the standard profile removal semantics, including role-binding
  clearance." `agent_check_removes_a_failed_profile_on_choice` asserts the
  profile disappears from the configuration but does not assert that a role
  binding to the removed profile is cleared.
- *Why it matters:* these are the executable surface of the declared contract;
  with no bounded test pin and no `IMPL.md` observation (F-1), a future change
  could drop or alter them without this compliance chain detecting a mismatch.
  None contradicts the observed core behavior — this is an evidence gap, not a
  behavioral conflict.
- *Arbitration options:* (a) pin bounded tests for each facet (check-side fixed
  prompt assertion, exhausted-stdin no-execution, `--json` single result object,
  role-binding clearance on removal) and document them in `IMPL.md`; (b) record
  a reasoned exception that the core acknowledgement/verification/removal/ignore
  behavior is the retained observation and the listed facets are covered by the
  shared runtime verification and the setup-side constant; (c) approved
  deferral with an owner.

**F-3 (LOW, transient) — The change's source material is present in the working
tree but not all of it is committed at review time.**
- *Locator:* working-tree state inspection (state only, not Git history): on
  branch `sandboxed-external-agents`, `maerg/src/wizard.rs` and
  `maerg/src/task_commands.rs` are modified relative to `HEAD`;
  `maerg/src/cli.rs` is not. This review record is untracked until the
  implementation/finalization step commits the accepted change.
- *Observed:* the source carrier for `agent check` / role-list behavior
  (`maerg/src/wizard.rs`, per the chain task description) is modified but
  uncommitted. This review reads no implementation source; the note is state
  observation only.
- *Why it matters:* an arbitrator inspecting the committed state would not see
  the source change until it is committed; the "current" working tree this
  review inspects is not yet the "committed" state.
- *Arbitration options:* none beyond the normal verification + commit flow;
  recorded for state clarity only.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `agent_check`, `wizard`, and `agent` suites are run
  automatically by Kvist in the verification sandbox after this turn, and no
  result from that run is asserted here.
- The runbook precondition to run the documented quality gate and
  `kvist doctor .` and resolve any non-`current` state was not executed in
  this context.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- The security-audit stage of this chain is completed per the queue, but its
  report is not in this reviewer's permitted read set; this review neither
  relies on nor repeats its conclusions.
- Optional native schemas are not referenced by `CONTRACT.md` for the `agent
  check` surface and therefore count as no evidence.
- All "observed" statements are attributed to the bounded test evidence or to
  `IMPL.md`; no implementation source was inspected, and the working-tree note
  in F-3 is state inspection only, not Git history.
- `maerg/src/cli.rs` (unmodified in the working tree) is the source carrier the
  prior chain referenced for the interactive command surface; this review does
  not inspect it and does not assert which source file ultimately dispatches
  `agent check`.

## Discrepancy summary

| ID | Finding | Status | Severity |
|----|---------|--------|----------|
| F-1 | No `IMPL.md` observation of `agent check` / role-list behavior (observed-behavior carrier absent) | mismatched | medium |
| F-2 | Contract-specific facets (check fixed prompt, exhausted stdin, `--json` single object, role-binding clearance) unpinned in bounded evidence | underspecified | low |
| F-3 | Change's source material uncommitted at review time | approved-deferred (state clarity) | low |

**Total discrepancies requiring arbitration:** 3 (F-1 medium; F-2, F-3 low)
**Satisfied clauses:** C-1…C-7 (compliant), C-8 (partially compliant — see F-2)

## Arbitration required

The project architect owns arbitration and may request redesign and
implementation changes, explicitly approve an intent-document change, or record
a reasoned exception or approved deferral. This review does **not** modify
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or `IMPL.md` to erase any
mismatch, and it does not treat external process success as acceptance. The
required decisions, in priority order, are:

1. **F-1 (MEDIUM):** Decide how the observed `agent check` / role-list behavior
   is preserved in `IMPL.md` — a fresh clean-slate observation pass (then re-run
   this compliance pass) or a reasoned exception that the bounded
   `agent_check.rs`/`wizard.rs` suites are the retained observation.
2. **F-2 (LOW):** Decide whether the four contract-specific facets (check-side
   fixed prompt, exhausted-stdin no-execution, `--json` single result object,
   role-binding clearance on removal) are pinned in bounded tests and
   documented, receive a reasoned exception referencing the shared runtime
   verification and the setup-side constant, or are deferred with an owner.
3. **F-3 (LOW):** No action beyond the normal verification + commit flow;
   recorded for state clarity only.

This record, the `IMPL.md`, and any approved arbitration decision are to be
retained in version control. No raw provider transcript, credential, secret, or
temporary review workspace is retained.
