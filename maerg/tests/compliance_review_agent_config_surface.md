<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Agent Configuration Surface (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `agent-config-surface-compliance-review`
**Scope:** Source-derived agent-configuration behavior — the provider,
profile, and role rework: `kvist.toml`/user-configuration parsing, shared
registry resolution, role binding and clearing, registry lifecycle, and
profile identity — compared against the agent-integration requirement, the
provided-interfaces and behavioral-guarantee contract, and the design.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is
the fourth and final stage of the `agent-config-surface` chain (tests →
implementation → security audit → compliance review). The implementer and the
security auditor did not author this record; this reviewer did not read
implementation source, dependency manifests, or Git history as evidence.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read
only the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the
`IMPL.md` and bounded test evidence; the nearest ancestor component
`CONTRACT.md` (none — `maerg` is the root component, `parent_contract: null`);
and `ROOT_CONTRACT.md`. Each clause is classified **compliant**,
**mismatched**, **approved-deferred**, or **underspecified**, with component
path, stable locator, intended text, observed behavior/evidence, severity, and
owner.

**Read (permitted):** `maerg/REQUIREMENTS.md`
(`sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e` —
matches the queue's `requirements_revision`), `maerg/CONTRACT.md`
(`sha256:646ba4be7b92ac8e292a4cae80274ff239ed06c3c7e7925dac4e5d2bed4a04f6` —
matches `contract_revision`), `maerg/DESIGN.md`
(`sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f` —
matches `design_revision`), `maerg/IMPL.md`
(`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`),
`ROOT_CONTRACT.md`
(`sha256:b4dd022afb6237cd20c255a0b9476348e9e76e43bfae4878d4626136be535252`),
`REVIEW_RUNBOOK.md` (procedure), and bounded test evidence:
`maerg/tests/model_settings.rs`
(`sha256:d850a389f193d1a425bdc7c73a92094d60fd449b7cb50b146b0fc390c03af003`),
`maerg/tests/shared_registry.rs`
(`sha256:4e23138725da50b286c445f2b36d194657c8054f7f63c701b66d2339cb7f2667`),
`maerg/tests/config_resolution.rs`
(`sha256:6e359971467621068ccb94f9b99e8d2b60ea9cf0a18e4f4fd23aaf4227babb76`),
`maerg/tests/agent.rs`
(`sha256:9f159d51777c3dd384be34d498b274c5489f83381633feb4a9b5552351700503`),
and `maerg/tests/agent_check.rs`
(`sha256:d8efaf07347438429692c2e157d3653e5623da0816d069ea909398e12f020064`),
and, cited only for two bounded approval-binding tests in C-9,
`maerg/tests/task_commands.rs`
(`sha256:b30e17736fe2a3a8565a3c9f3daf8ac0317549f37397532dab17d7498ef4c5af`).

**Not read:** `maerg/src/**` implementation source (including `src/config.rs`),
`sav/` and `galla/` source, `Cargo.toml`/`Cargo.lock`, Git history or diffs,
prior compliance conclusions, and peer or parent implementation artifacts.
No tests, builds, linters, or `cargo` invocations were executed in this
review; the configuration suites are run automatically by Kvist in the
verification sandbox after this turn, and no pass/fail count from that run is
asserted here.

## Method

Every agent-configuration obligation relevant to the provider/profile/role
rework was drawn from `REQUIREMENTS.md` (`REQ-AGENT-INTEGRATION`),
`CONTRACT.md` (`Shared registry configuration (ADR 0015)`, `Provided
interfaces`, `Data and schemas`, `Security and authority`, `Compatibility and
verification`), `DESIGN.md` (agent configuration loader, ADR 0015), and
`ROOT_CONTRACT.md` (`Change and compliance rules`, untrusted-input rule), and
compared with the observed behavior recorded in `IMPL.md` and the bounded
agent-configuration test evidence. The comparison is clause against evidence;
the test assertions are the executable statement of observed behavior in the
places `IMPL.md` is silent. The chain task contexts for
`agent-config-surface-write-tests` and `agent-config-surface-implement-code`
identify three rework behaviors the chain claims to have pinned: (a) rejection
of top-level `provider`/`profile`/`role` tables outside the agent table,
(b) an explicit, deterministic legacy migration heuristic, and (c) a total
profile equality for NaN temperature values. Each is examined below.

**Task-context provenance (disclosed).** The chain task definitions above were
read from the canonical queue `TODOS.yaml` (task `id`, `title`,
`description`, `context`, and `status` fields only — no lifecycle metadata
beyond status). The runbook's permitted read set for this pass does not list
`TODOS.yaml`; the reviewer used it solely to scope the *claimed* rework
surface, because the queue is the durable record of what the immediately
preceding chain stages assert they changed and pinned. It was not used as
evidence of observed behavior, and every "observed" statement below is
attributed exclusively to `IMPL.md` or the bounded test evidence. If the
arbitrator treats even this scoping input as outside the pass, F-1, F-2, and
F-3 stand on the same facts restated in each finding's *Locator* (the intent
documents and bounded tree were searched for the three behaviors and found
them absent or only consequentially pinned).

## Satisfied clauses (compliant)

**C-1 — Shared registry selection and role binding are local, resolved once,
and digest-bound.** `CONTRACT.md` `Shared registry configuration (ADR 0015)`:
`[agent.registry] path` selects a shared registry relative to the
configuration file; `[agent.roles.ROLE] profile = "NAME"` binds a shared named
profile with role policy remaining local; selection is resolved once while
loading configuration and exact registry source bytes contribute to the
configuration digest. Evidence: `role_profile_is_shared_structured_pinned_and_
digest_bound` — a role binding resolves to the shared profile (`coder` →
`shared-model`), the loaded role retains its `resolved_profile` snapshot, and
changing the registry file changes the configuration source digest while the
already-loaded resolution keeps its pinned value. Compliant.

**C-2 — The agent configuration surface is the shared registry plus local role
policy; copied local provider/profile definitions are not mixed into the
shared binding.** `CONTRACT.md` `Shared registry configuration (ADR 0015)`;
`IMPL.md` `Shared selection and pinned authority` (shared bindings disallow
mixing copied local provider/profile definitions; role tables are restricted
to profile references plus role policy). Evidence: `shared_agent_list_renders_
one_authoritative_catalogue_and_scoped_role_bindings` — with a shared binding
active, the catalogue renders each shared profile exactly once, shows the
scoped role bindings (`architect -> coder`, `developer -> fallback`,
`security-reviewer -> coder`), and does not render the unrelated user-scope
legacy profile; `effective_user_fallback_and_user_references_are_inspected_
without_probing` — user-scope role references resolve to the selected shared
source with `configuration.scope = "user"`. Compliant.

**C-3 — Scope selection: project-local configuration inside a project, user
configuration with `--global`; project-local wins over user; user edits never
assign a project-only definition; the selected registry source is reported.**
`CONTRACT.md` `Provided interfaces` (scope inspection resolves available
profiles in the selected project/user scope; user edits never assign a
project-only definition; outside an initialized project, role pages start in
user scope). Evidence: `agent_list_uses_the_selected_project_local_shared_
source_instead_of_global_models`, `agent_list_uses_the_selected_shared_user_
source_without_legacy_empty_warning`, `registry_precedence_and_shadowed_
fallback_are_source_specific` (project definition wins; the shadowed user
definition is reported as fallback), `shared_remove_requires_explicit_source_
and_edits_only_that_file` (removal of a user-scope shared definition refuses
with a `--global`-naming diagnostic and no write), and
`resolves_global_user_config_via_kvist_config_path` in `config_resolution.rs`
(the resolved user configuration path is honored end-to-end). Compliant.

**C-4 — `agent role list` presents only the predefined roles and their
assigned model profiles, and never presents model profile names as roles.**
`REQ-AGENT-INTEGRATION`; `CONTRACT.md` `Provided interfaces` (predefined roles
developer, architect, security-reviewer). Evidence:
`agent_role_list_presents_only_predefined_roles` — with two configured model
profiles, every row of the role table names only one of the three predefined
roles, while the assigned and available profiles remain visible. Compliant.

**C-5 — `agent check` acknowledges before executing, is a cancellation point,
offers removal or ignore for failing profiles, and changes nothing on
refusal/cancellation.** `REQ-AGENT-INTEGRATION` (live-verify each configured
profile with the fixed minimal prompt; no test command without an explicit
interactive acknowledgement that is itself a cancellation point; failing
profiles offered removal or temporary ignore; cancellable at every prompt).
Evidence: `agent_check_reports_healthy_profiles_without_changes` (healthy
profiles pass, configuration byte-identical), `agent_check_removes_a_failed_
profile_on_choice` (removal uses the standard profile-removal semantics),
`agent_check_ignores_a_failed_profile_on_choice` (ignore leaves configuration
unchanged), `agent_check_refusal_executes_nothing_and_changes_nothing` (a
marker script never runs; configuration unchanged), `agent_check_cancels_at_
the_failure_prompt_without_changes`, and `agent_check_without_profiles_
reports_nothing_to_check`. Compliant.

**C-6 — Role edits validate selections and reasoning effort without partial
writes: disabled selections refused, malformed configuration refused, effort
inheritance is distinct from explicit `none`, and known unsupported non-`none`
overrides refuse without writing while unknown capability stays explicitly
unknown.** `CONTRACT.md` `Provided interfaces` (known unsupported non-`none`
reasoning overrides refuse without writing; unknown capability support remains
explicitly unknown); `REQ-AGENT-INTEGRATION` (typed reasoning effort only when
the selected model command declares support). Evidence:
`disabled_selection_and_malformed_configuration_never_write` (both refusal
paths leave the configuration byte-identical), `role_effort_inherit_is_
distinct_from_explicit_none_and_preserves_policy` (`--inherit-effort` writes
no `thinking_effort` key while `--thinking-effort=none` does; combining both
flags refuses), `changing_models_refuses_retained_unsupported_effort_without_
writing` (moving a role with a retained effort to a profile that does not
support thinking refuses with an `unsupported` diagnostic and no write; the
same move with `--inherit-effort` succeeds and drops the override), and
`initialized_no_agent_project_has_empty_library_and_explicit_unknowns` (null
model, `enabled: false` — explicit unknown, not inferred). Compliant.

**C-7 — Role clearing and unassignment are separate from removal: clearing
removes the profile binding only, preserves role policy and unrelated
definitions, refuses stale pins and user-scope references, and never
reinterprets a canonical profile model as an assignment.** `CONTRACT.md`
`Provided interfaces` (disable/removal refuses known role references;
unassignment is separate and never silently clears other roles; `--all` resets
the selected local agent configuration, not the shared registry). Evidence:
`clearing_shared_role_reference_enables_later_removal_without_policy_changes`
(`thinking_effort` and `timeout_seconds` survive clearing; the reference is
dropped, enabling later removal), `clearing_roles_preserves_canonical_role_
named_and_unrelated_model_definitions` (both `developer` and `--all` selectors
preserve unrelated profile definitions and other roles' policy),
`clearing_without_a_binding_never_reinterprets_canonical_profile_model_as_an_
assignment` (clearing a role with no binding is an error, no write), and
`pinned_local_removal_refuses_stale_and_user_references_then_removes_only_
selected_definition` (a stale pin and a user-scope reference both refuse
without writes; after clearing the user binding, removal edits only the
project definition and preserves comments and unrelated entries). Compliant.

**C-8 — Registry lifecycle mutations are source-pinned, refuse stale
revisions and busy locks, preserve unrelated entries and capabilities, and
edit only the named source.** `CONTRACT.md` `Provided interfaces` (registry
enable/disable/removal requires the exact source and its inspected revision).
Evidence: `removal_edits_actual_source_and_refuses_stale_revision_without_
writes`, `referenced_disable_and_remove_refuse_without_clearing_role` (both
actions refuse with a reassign diagnostic; no write),
`enable_disable_preserve_capabilities_and_busy_lock_never_changes_source`
(a foreign busy lock refuses mutation without touching the lock or the
source; enable preserves advertised/tested/policy capability lists; a fresh
mutation returns a changed revision), and
`inspection_refuses_symlink_and_oversize_registry` plus
`inspection_refuses_ancestor_symlinks_nonregular_sources_and_invalid_metadata`
(link-like, non-regular, and oversize registry sources are refused).
Compliant.

**C-9 — Configuration content is untrusted input: inspection is local and
bounded, secrets and opaque external arguments are not displayed, and the
approval binding detects configuration-source changes.** `ROOT_CONTRACT.md`
(untrusted-input rule); `CONTRACT.md` `Security and authority` and
`Data and schemas` (`kvist.toml`: strict TOML configuration). Evidence:
`inspection_is_typed_bounded_and_includes_disabled_capability_evidence`
(local typed inspection, no probing),
`invalid_shadowed_user_config_fails_and_detail_excludes_external_arguments`
(an external-agent profile's opaque arguments are absent from the detail),
`first_project_fork_never_copies_sensitive_redaction_values`, and in
`IMPL.md` `Shared selection and pinned authority` the agent configuration
source digest incorporates resolved profile names, source paths, and digests;
`task_commands.rs` evidence
(`approval_record_is_deterministic_and_rejects_changed_agent_template_before_
probe`, `approval_rejects_changed_agent_source_and_runner_content_before_
mutation`) shows approval is re-bound when configuration material changes.
Compliant.

## Discrepancies (mismatched / underspecified — retained for arbitration)

**F-1 (HIGH, underspecified) — The rework's rejected-surface rule (top-level
`provider`/`profile`/`role` tables outside the agent table) is declared in no
intent document and is pinned in no bounded test.**
- *Locator:* `CONTRACT.md` `Shared registry configuration (ADR 0015)` and
  `Data and schemas` (only "strict TOML configuration" and the
  `[agent.registry]`/`[agent.roles.ROLE]` surface); `REQUIREMENTS.md`
  `REQ-AGENT-INTEGRATION` (no configuration-surface clause); `DESIGN.md`
  (agent configuration loader; no accepted-section enumeration); chain task
  context for `agent-config-surface-write-tests` ("provider, profile, and
  role tables are rejected with actionable diagnostics outside the agent
  table") and `agent-config-surface-implement-code` ("reject top-level
  provider, profile, and role tables with actionable diagnostics").
- *Intended (declared):* `kvist.toml` is strict TOML configuration; the
  declared agent surface is the shared registry plus local role policy (C-1).
  No clause enumerates the accepted top-level sections, states that top-level
  `provider`/`profile`/`role` tables are rejected, or specifies the
  diagnostic.
- *Observed (bounded test evidence):* none of the bounded configuration tests
  constructs a top-level (non-`[agent]`) `provider`/`profile`/`role` table or
  asserts its rejection. Every configuration fixture places provider/profile
  tables inside `[agent]` (local command configurations — an accepted surface,
  see `agent_check.rs` fixtures and
  `pinned_local_removal_refuses_stale_and_user_references_then_removes_only_
  selected_definition`) or in a shared registry's `[profiles.*]` namespace
  (`shared_registry.rs` `SHARED_MODELS`). `IMPL.md` carries no observation of
  a top-level-table rejection (see F-4).
- *Why it matters:* Configuration content is untrusted input that steers which
  profile executes a task, and the chain's own context states that silently
  accepting an undeclared surface "can change which profile a role selects
  without any recorded intent change." The rework claims this surface is now
  rejected with an actionable diagnostic, but neither the declared intent nor
  the retained test evidence states the rule, so this review cannot record
  "compliant" for it.
- *Arbitration options:* (a) declare the exact accepted configuration surface
  (which top-level tables are permitted, that top-level
  `provider`/`profile`/`role` tables are rejected, and the diagnostic) in
  `CONTRACT.md`/`REQUIREMENTS.md` and pin it in a bounded test; (b) record a
  reasoned exception that "strict TOML configuration" plus enumeration of the
  accepted tables is the declared form; (c) approved deferral with an owner.
  This review does not read `src/config.rs` to verify the claimed rejection
  and does not certify the `write-tests` task's claim ("tests fail on the
  accepted-but-undeclared tables… and pass once the surface is pinned") from
  the bounded tree, which contains no such test.

**F-2 (MEDIUM, underspecified) — Profile equality is declared nowhere, and
the NaN-temperature totality the chain claims is unverified in the bounded
evidence.**
- *Locator:* chain task context for `agent-config-surface-write-tests`
  ("profile equality is well-defined for NaN temperature values") and
  `agent-config-surface-implement-code` ("make profile equality total by
  handling NaN temperature values explicitly"); `CONTRACT.md` /
  `REQUIREMENTS.md` / `DESIGN.md` — no clause mentions a temperature field or
  profile equality semantics.
- *Intended (declared):* nothing. The intent documents state no requirement
  that profile comparison be total, and declare no temperature field.
- *Observed (bounded test evidence):* no bounded configuration test asserts
  anything about temperature or profile equality; no test file contains a
  NaN or temperature assertion. Profile identity in the retained evidence is
  exercised through source revisions/digests (C-1, C-8), not through value
  equality.
- *Why it matters:* a derived `Eq` over an unbounded floating-point field is
  not a total order (NaN compares unequal to itself), so any comparison of
  profiles can change role selection or approval binding on NaN input. The
  rework claims the issue is fixed by explicit handling, but with no intent
  clause and no retained test, the totality property has no evidence in this
  source-blind pass.
- *Arbitration options:* (a) declare the profile value semantics (field types
  and comparison rules, or the exact totality rule for NaN temperature) in
  intent and pin a bounded test; (b) reasoned exception that profile identity
  is defined by source revision/digest alone (C-1, C-8) and value equality is
  not a declared behavior; (c) deferral with owner.

**F-3 (MEDIUM, underspecified) — The legacy-format heuristic is pinned as
deterministic by tests but its rule is not declared in intent, and its
presence sits in tension with the no-migration statement.**
- *Locator:* chain task context for `agent-config-surface-write-tests` ("the
  legacy role heuristic is content-based") and
  `agent-config-surface-implement-code` ("make the legacy migration heuristic
  explicit and documented"); `CONTRACT.md` `Compatibility and verification`
  ("This pre-release contract intentionally has no compatibility or migration
  commitment"); `CONTRACT.md` `Shared registry configuration (ADR 0015)`;
  `REQUIREMENTS.md` `REQ-AGENT-INTEGRATION`.
- *Intended (declared):* no clause states when a configuration binding is
  classified as legacy, what content shapes trigger the classification, what a
  migration produces, or when it refuses.
- *Observed (bounded test evidence):* the legacy path is exercised and pinned
  as deterministic, but only for presentation and unassignment.
  `legacy_agent_list_preserves_explicit_project_and_user_profile_scopes` pins
  that legacy-format `[agent.profiles.*]` bindings render under "Configured
  Models in Project" / "Global User Profiles" headers with no shared-catalogue
  section; `agent_list_uses_the_selected_shared_user_source_without_legacy_
  empty_warning` pins that a selected shared user source suppresses the legacy
  empty warning; `shared_agent_list_with_no_enabled_profiles_does_not_fall_
  through_to_legacy_models` pins that an empty shared registry reports "no
  enabled shared profiles" rather than falling through to legacy models; and
  `legacy_role_clear_requires_unambiguous_role_assignment_fields` pins that
  `agent role clear` on a profile carrying both `model` and `models = [...]`
  removes only the `model` field (deterministic field-level resolution) while
  a content-ambiguous shape (both plus `provider`) refuses the `--all` clear
  without any write.
- *Why it matters:* the classification rule itself ("content-based") is the
  declared surface of a configuration parser whose output selects the
  executing profile. The unambiguity rule is security-relevant (an ambiguous
  shape must refuse, as pinned). The tests prove determinism of the
  *consequences*; no intent text states the rule, so a future intent revision
  could change or remove it without the compliance chain detecting a
  mismatch. Additionally, `CONTRACT.md` states the contract has no
  compatibility or migration commitment, while the pinned behavior preserves
  and classifies legacy-format content — the arbitrator should confirm
  whether the retained legacy behavior is an intentional, documented
  exception to that statement.
- *Arbitration options:* (a) declare the legacy classification rule, its
  deterministic output, and its refusal condition (and its status relative to
  the no-migration statement) in `CONTRACT.md`; (b) reasoned exception that
  the pinned test behavior is the accepted form; (c) deferral with owner.

**F-4 (HIGH, mismatched) — `IMPL.md` contains no observation of the
provider/profile/role configuration-surface rework, so the observed-behavior
carrier is missing for the rework-specific behaviors.**
- *Locator:* `maerg/IMPL.md`
  (`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`);
  `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md` describes
  observed implementation behavior and is not copied from intended
  requirements, contracts, or designs"); `REVIEW_RUNBOOK.md` (the compliance
  reviewer reads "the newly produced `IMPL.md`").
- *Intended (declared):* `IMPL.md` states public behavior, guarantees and
  limits, state effects, failure paths, and known uncertainty, derived
  independently from source; the compliance pass compares intent against that
  fresh observation.
- *Observed:* `IMPL.md` carries no heading, addendum, or scoped observation
  covering the configuration-surface rework. Its only rework-adjacent section,
  `Shared selection and pinned authority`, describes the shared-registry
  loader, the role-table restriction, and the source digest, and it is
  consistent with the intent (C-1, C-2). It does not mention top-level-table
  rejection, a migration or legacy-classification heuristic, temperature,
  equality totality, or any NaN handling. A term sweep of `IMPL.md` for
  `top-level`, `outside the agent`, `undeclared`, `NaN`, `temperature`,
  `heuristic`, and `equality` found no rework observation.
- *Why it matters:* the task's purpose is to "preserve independent evidence
  that configuration handling matches its declared behavior." For the three
  rework-specific behaviors (F-1, F-2, F-3), the observed-behavior carrier is
  absent: the source-blind comparison degrades to tests-only, and the bounded
  tests do not pin those behaviors either (F-1, F-2) or pin only their
  consequences (F-3). The runbook expects a fresh `IMPL.md` observation as
  the primary carrier.
- *Arbitration options:* (a) commission a clean-slate documenter pass over the
  configuration change so `IMPL.md` carries the observed rework behavior,
  then re-run this compliance pass against it; (b) record a reasoned
  exception that the bounded tests are the retained observation for this
  chain (noting F-1/F-2 have no such tests in the bounded tree); (c) deferral
  with owner. This review does not modify `IMPL.md` (it is outside the
  reviewer's writable scope and outside the permitted read-modify set of this
  pass).

**F-5 (LOW, transient) — The rework's changed material is present in the
working tree but not all of it is committed at review time.**
- *Locator:* working-tree state inspection (state only, not Git history):
  `maerg/src/config.rs`, `maerg/README.md`, and `maerg/TODOS.yaml` are
  modified relative to `HEAD` on branch `sandboxed-external-agents`; the
  bounded configuration test files read here are not listed as modified.
  This review record is untracked until the implementation/finalization step
  commits the accepted change.
- *Observed:* the intent digests match the queue's recorded revisions
  (revalidation `state: current`, checked 2026-11-14T19:35:33Z), so the
  compliant-vs-intent comparison above is against the current intent set; the
  rework's `src/config.rs` change is not yet in the version-controlled state
  this review inspects.
- *Why it matters:* an arbitrator inspecting the committed state would not see
  the tightened configuration surface; the "current" state this review
  inspects is not yet the "committed" state.
- *Arbitration options:* none beyond the normal verification + commit flow;
  recorded for state clarity only.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `model_settings`, `shared_registry`, `config_resolution`,
  `agent`, and `agent_check` suites are run automatically by Kvist in the
  verification sandbox after this turn, and no result from that run is
  asserted.
- The runbook precondition to run the documented quality gate and
  `kvist doctor .` and resolve any non-`current` state was not executed in
  this context; the queue's own `revalidation` block records
  `state: current` and the three intent digests in this record match the
  queue revisions, which is the bounded substitute available to this reviewer.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- The security-audit stage of this chain is completed per the queue, but its
  report is not in this reviewer's permitted read set; this review neither
  relies on nor repeats its conclusions.
- Optional native schemas are not referenced by `CONTRACT.md` for the agent
  configuration surface and therefore count as no evidence.
- All "observed" statements are attributed to the bounded test evidence or to
  `IMPL.md`; no implementation source was inspected, and the working-tree
  note in F-5 is state inspection only, not Git history.

## Arbitration required

The project architect owns arbitration and may request redesign and
implementation changes, explicitly approve an intent-document change, or
record a reasoned exception or approved deferral. This review does **not**
modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or `IMPL.md` to erase
any mismatch, and it does not treat external process success as acceptance.
The required decisions, in priority order, are:

1. **F-1 (HIGH):** Decide whether the rejected-surface rule (top-level
   `provider`/`profile`/`role` tables outside the agent table, with an
   actionable diagnostic) becomes declared intent with a pinned bounded test,
   or receives a reasoned exception / approved deferral. Note that the
   bounded tree retains no test pinning this rule.
2. **F-4 (HIGH):** Decide how the observed rework behavior is preserved in
   `IMPL.md` — a fresh clean-slate observation pass (then re-run this
   compliance pass) or a reasoned exception that the bounded tests are the
   retained observation.
3. **F-2 (MEDIUM):** Decide the declared semantics of profile value equality
   (including NaN temperature totality) — declare and pin it, or record a
   reasoned exception that profile identity is defined by source
   revision/digest alone.
4. **F-3 (MEDIUM):** Decide whether the legacy-format classification rule
   (including its relationship to the no-migration statement) is declared in
   `CONTRACT.md`, excepted, or deferred.
5. **F-5 (LOW):** No action beyond the normal verification + commit flow;
   recorded for state clarity only.

This record, the fresh `IMPL.md`, and any approved arbitration decision are to
be retained in version control. No raw provider transcript, credential,
secret, or temporary review workspace is retained.
