<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Discovery Hardening (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `discovery-hardening-compliance-review`
**Scope:** Source-derived discovery behavior — workspace-namespace
classification, nested-subproject handling, and artifact-projection error
propagation — compared against the project-layout requirements, the
provided-interfaces and behavioral-guarantee contract, and the design.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is
the fourth and final stage of the `discovery-hardening` chain (tests →
implementation → security audit → compliance review). The implementer and the
security auditor did not author this record; this reviewer did not read
implementation source, dependency manifests, or Git history as evidence.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read
only the target `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`; the newly
produced `IMPL.md` and bounded test evidence; the nearest ancestor component
`CONTRACT.md` (none — `maerg` is the root component, `parent_contract: null`);
and `ROOT_CONTRACT.md`. Each clause is classified **compliant**, **mismatched**,
**approved-deferred**, or **underspecified**, with component path, stable
locator, intended text, observed behavior/evidence, severity, and owner.

**Read (permitted):** `maerg/REQUIREMENTS.md`
(`sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e` —
matches the queue's `requirements_revision`), `maerg/CONTRACT.md`
(`sha256:646ba4be7b92ac8e292a4cae80274ff239ed06c3c7e7925dac4e5d2bed4a04f6` —
matches `contract_revision`), `maerg/DESIGN.md`
(`sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f` —
matches `design_revision`), `maerg/IMPL.md`
(`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`),
`ROOT_CONTRACT.md`, `REVIEW_RUNBOOK.md` (procedure), and bounded test evidence
`maerg/tests/discovery.rs`
(`sha256:d02cedbf2b4db8e390550a9cf4c57669ce0e63707821c48b35b235f60ff30587`)
and `maerg/tests/discovery_limits.rs`
(`sha256:5e91e2a01ce19c57db58c26fc7592ad07990c35ab619e9cf18f5d9580703c7b7`).

**Not read:** `maerg/src/**` implementation source, `Cargo.toml` /
`Cargo.lock`, Git history or diffs, prior compliance conclusions, and peer or
parent implementation artifacts. No tests, builds, linters, or `cargo`
invocations were executed in this review; the `discovery` suites are run
automatically by Kvist in the verification sandbox after this turn, and no
pass/fail count from that run is asserted here.

## Method

Every discovery obligation relevant to the hardening was drawn from
`REQUIREMENTS.md` (`REQ-DISCOVERY-STATUS`, `REQ-ARTIFACT-MODEL`,
`REQ-PROJECT-LAYOUT`), `CONTRACT.md` (`Behavioral guarantees`, `Provided
interfaces`, `Command surface`, `Errors and failure semantics`), `DESIGN.md`
(project model and interaction claims), and `ROOT_CONTRACT.md` (`Change and
compliance rules`), and compared with the observed behavior recorded in
`IMPL.md` and the bounded discovery test evidence. The comparison is clause
against evidence; the test assertions are the executable statement of observed
behavior in the places `IMPL.md` is silent.

## Satisfied clauses (compliant)

**C-1 — Discovery is lexical, bounded, and never follows link-like paths.**
`CONTRACT.md` `Behavioral guarantees`; `REQ-DISCOVERY-STATUS`. Bounded test
evidence: `rejects_component_trees_deeper_than_the_fixed_bound` (error text
"maximum depth"), `rejects_link_like_descendants_without_following_them`
(cyclic symlink rejected, error text "link-like component path"), and
`discovery_reports_each_resource_limit_specifically` in `discovery_limits.rs`
(five distinct limit diagnostics: depth, scanned directories, recognized
components, entries per directory, relative-path bytes). Compliant.

**C-2 — A directory is a child component iff at least one required adjacent
artifact exists; ordinary directories are transparent, not components.**
`CONTRACT.md` `Behavioral guarantees`; `REQ-ARTIFACT-MODEL`. Evidence:
`identifies_incomplete_components_without_treating_ordinary_directories_as_
components`, `discovers_complete_nested_components_in_lexical_order`, and
`discovery_allows_components_below_ordinary_directories` (components below
ordinary directories are discovered; transparent directories do not interrupt
parent lookup — the same rule `REVIEW_RUNBOOK.md` preconditions rely on).
Compliant.

**C-3 — Five-artifact set and reporting order; malformed layouts are
classified, not silently accepted.** `CONTRACT.md` `Behavioral guarantees`
(order `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`,
`IMPL.md`); `REQ-ARTIFACT-MODEL`. Evidence: `identifies_malformed_artifact_
layouts` (a directory where the task queue file belongs is reported
`ArtifactStatus::Invalid(Directory)`), and every fixture builds the same
adjacent five-artifact set. Compliant.

**C-4 — Discovery is deterministic within configured hard bounds and does not
modify project files.** `REQ-DISCOVERY-STATUS`; `CONTRACT.md` `Behavioral
guarantees` ("Discovery is lexical, bounded…"). Evidence: lexical ordering is
asserted in `discovers_complete_nested_components_in_lexical_order`
(`.`, `alpha`, `zebra` — root first, then lexicographic), limits are supplied
via `discover_with_limits` and read from loaded configuration
(`discovery_configuration_defaults_and_invalid_ranges_are_enforced`), and no
test observes a project mutation as an effect of discovery. Compliant as
declared; the *implementation* property of loading configuration once is
separately recorded as F-3.

**C-5 — Status reports surface the discovery error rather than hiding it.**
`REQ-DISCOVERY-STATUS` ("each non-valid root artifact with its exact reason,
any project-root diagnostic and discovery error, and a concrete next
action"); `CONTRACT.md` `Provided interfaces` guidance rules ("a non-current
project state is never shown bare"). The discovery-error text is propagated
verbatim through the report path in the bounded evidence (the surfaced
error is asserted to contain the link-like path reason in
`artifact_projection_surfaces_discovery_errors`). Compliant at the report
surface; the *projection* propagation obligation itself is separately
recorded as F-2.

**C-6 — Project layout places maerg root artifacts, workspace manifest,
source, and root integration tests in one component directory; `sav` and
`galla` are complete child components.** `REQ-PROJECT-LAYOUT`; `CONTRACT.md`
`Boundary and ownership`. The layout the discovery behavior operates on is
the one the tests assume (root `kvist.toml` plus peer component directories,
each carrying the five-artifact set — `discovers_top_level_peer_components_
when_root_is_dot`). Compliant at the declared level; this review does not
re-verify the physical layout beyond what the bounded evidence carries.

## Discrepancies (mismatched / underspecified — retained for arbitration)

**F-1 (HIGH, underspecified) — The workspace-namespace classification
semantics the hardening pins are not declared in any intent document.**
- *Locator:* `REQUIREMENTS.md` `REQ-PROJECT-LAYOUT` and `REQ-DISCOVERY-STATUS`
  (no `component_root` clause); `CONTRACT.md` `Behavioral guarantees` and
  `Provided interfaces` (no workspace-root clause; `component_root` appears
  only in `repair --component-root` path options and "relative to the
  project's component root" path interpretation); `DESIGN.md` (no
  workspace-namespace classification claim).
- *Intended (declared):* Discovery MUST determine the configured recursive
  component hierarchy within configured hard bounds, and the layout MUST
  distinguish the maerg component directory from project vision/architecture/
  root-contract material outside it. Nothing states how a project's
  `component_root` setting classifies the project as a workspace namespace
  root, which normalization applies to that setting, or whether the component
  root is emitted as a root component.
- *Observed (test evidence):* `workspace_namespace_classification_reuses_
  loaded_configuration` pins that `component_root = "./"` — normalized to `.`
  by the loaded configuration — classifies the project as a workspace root, so
  the component root is *not* emitted as a root component and only nested
  components are discovered. The test's own doc comment records the former
  divergence (a raw re-parse comparing the literal `"./"` against `"."`) and
  the required fix. `nested_subproject_is_not_misclassified_as_workspace_
  root` pins that a nested directory carrying its own `kvist.toml` is a
  regular complete component, not a suppressed second workspace root.
  `workspace_classification_is_decoupled_from_unrelated_config_sections` pins
  that classification reads only `component_root` and is unaffected by the
  validity of unrelated sections (e.g., a malformed `test_policy`).
- *Why it matters:* The task's expected outcome is "a source-blind review
  records satisfied clauses, discrepancies, and required arbitration" of the
  source-derived behavior against *declared* behavior. These three
  classification rules are now executable and enforced, but they have no
  intent clause to be compared against: their only normative statement is the
  test doc comments. A future clean-slate documenter or reviewer cannot
  confirm them from `REQUIREMENTS.md`/`CONTRACT.md`/`DESIGN.md`, and an
  intent revision could remove them without the compliance chain detecting a
  mismatch. `ROOT_CONTRACT.md` requires recording intent-to-implementation
  discrepancies for explicit arbitration and forbids silently altering either
  artifact — this is such a discrepancy, retained here.
- *Arbitration options:* (a) add an explicit contract/requirements clause
  stating the classification rule (loaded `component_root` after normalization
  selects the namespace root; the root is not emitted as a component; nested
  `kvist.toml` subprojects are regular components; classification is
  independent of unrelated section validity) and revalidate; (b) record a
  reasoned exception that these are implementation-pinned semantics
  intentionally left out of the consumer-facing contract; (c) approved
  deferral with an explicit owner.

**F-2 (MEDIUM, underspecified) — The artifact-projection
discovery-error-propagation obligation is not a declared contract clause.**
- *Locator:* `CONTRACT.md` `Behavioral guarantees` (discovery guarantees
  stated about traversal only) and `Errors and failure semantics` (no
  artifact-projection clause); `DESIGN.md` `Interactions and state` ("Only a
  current root permits bounded component discovery. Each discovered component
  is then validated in stable artifact order" — describes the flow, not the
  failure mode of the projection's internal discovery).
- *Intended (declared):* Status reports MUST surface the discovery error
  (C-5). Beyond the report surface, no clause states what a *consumer* of
  discovery does when the second discovery performed during project-state
  artifact projection fails for a workspace-rooted project.
- *Observed (test evidence):* `artifact_projection_surfaces_discovery_errors`
  pins that the projection fails loudly: with a link-like path at the project
  root, `project_state::inspect` returns `Err` carrying the link-like error
  rather than `Ok` with a silently truncated artifact set. The test doc
  comment records the former behavior (the projection swallowed the error via
  `if let Ok` and continued).
- *Why it matters:* The fail-loud behavior is the security-relevant half of
  the hardening (a truncated artifact set can silently change downstream
  classification), yet the obligation exists only in the test and the
  write-tests task context. It is not comparable against intent, so
  "compliant" cannot be recorded for it.
- *Arbitration options:* (a) declare the clause in `CONTRACT.md` (a
  discovery failure inside project-state artifact projection is an error of
  the inspection, never a silently truncated result); (b) reasoned exception
  that the generic "status never hides discovery errors" guarantee (C-5) is
  accepted as the declared form; (c) deferral with owner.

**F-3 (LOW, underspecified) — Single-parse discovery (classification reusing
the loaded configuration) is not a declared property.**
- *Locator:* `CONTRACT.md` / `DESIGN.md` / `REQUIREMENTS.md` — no clause
  states that discovery loads project configuration exactly once or that
  classification must reuse the loaded (normalized) configuration.
- *Intended (declared):* Deterministic discovery within hard bounds
  (`REQ-DISCOVERY-STATUS`); nothing more specific.
- *Observed (test evidence):* `workspace_classification_is_decoupled_from_
  unrelated_config_sections` and `workspace_namespace_classification_reuses_
  loaded_configuration` together pin that classification observes the
  *normalized* value of the loaded configuration, which is the observable
  consequence of single-parse behavior (a second raw parse of the file cannot
  reproduce the loaded value).
- *Why it matters:* "Deterministic" is declared but "single-parse / no
  re-parse divergence" is not; the pinned tests are the only statement of the
  property. Low severity because the observable consequence is covered by
  F-1's classification rule, but the root cause (duplicated parsing) has no
  intent text to prevent a regression to it.
- *Arbitration options:* fold into F-1's clause ("classification consumes the
  loaded configuration"), or record no separate intent.

**F-4 (HIGH, mismatched) — `IMPL.md` contains no observation of the
discovery-hardening behavior, so the observed-behavior carrier is missing.**
- *Locator:* `maerg/IMPL.md` `sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`;
  `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md` describes
  observed implementation behavior and is not copied from intended
  requirements, contracts, or designs"); `REVIEW_RUNBOOK.md` (the compliance
  reviewer reads "the newly produced `IMPL.md`").
- *Intended (declared):* `IMPL.md` states public behavior, guarantees and
  limits, state effects, failure paths, and known uncertainty, derived
  independently from source; the compliance pass compares intent against that
  fresh observation.
- *Observed:* `IMPL.md` carries no heading, addendum, or scoped observation
  covering the discovery-hardening change. Its scoped observations are dated
  up to 2026-11-08; the chain's implementation completed 2026-11-19. There is
  no mention of `component_root` classification, workspace-namespace roots,
  the artifact-projection error path, or the discovery interfaces the tests
  exercise. The only observed-behavior evidence available to this source-blind
  review is therefore the bounded test assertions themselves.
- *Why it matters:* The task's purpose is to "preserve independent evidence
  that discovery matches its declared behavior." With `IMPL.md` silent, the
  chain's observed behavior is preserved in test doc comments — which
  describe the *required* fix more than the *observed* implementation — and
  the source-blind comparison degrades to tests-only. The runbook expects a
  fresh `IMPL.md` observation as the primary carrier.
- *Arbitration options:* (a) commission a clean-slate documenter pass over the
  discovery change so `IMPL.md` carries the observed behavior, then re-run
  this compliance pass against it; (b) record a reasoned exception that the
  bounded tests are the retained observation for this chain; (c) deferral
  with owner. This review does not modify `IMPL.md` (it is outside the
  reviewer's writable scope and outside the permitted read-modify set of this
  pass).

**F-5 (LOW, transient) — The chain's changed material is present in the
working tree but uncommitted at review time.**
- *Locator:* working-tree state inspection (state only, not Git history):
  `maerg/src/discovery.rs`, `maerg/src/project_state.rs`, and
  `maerg/tests/discovery.rs` are modified relative to `HEAD`; this review
  record will itself be untracked until the implementation/finalization step
  commits the accepted change.
- *Observed:* the intent digests match the queue's recorded revisions
  (revalidation `state: current`), so the compliant-vs-intent comparison
  above is against the current intent set; the hardening's source and test
  changes are not yet in the version-controlled state.
- *Why it matters:* an arbitrator inspecting the committed state would not
  see the hardened behavior; the "current" state this review inspects is not
  yet the "committed" state.
- *Arbitration options:* none beyond the normal verification + commit flow;
  recorded for state clarity only.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `discovery` and `discovery_limits` suites are run
  automatically by Kvist in the verification sandbox after this turn, and no
  result from that run is asserted.
- The runbook precondition to run the documented quality gate and
  `kvist doctor .` and resolve any non-`current` state was not executed in
  this context; the queue's own `revalidation` block records
  `state: current` (checked 2026-11-14T19:35:33Z) and the three intent digests
  in this record match the queue revisions, which is the bounded substitute
  available to this reviewer.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- Claims about enforcement internals (exact error propagation paths, the
  former `if let Ok` swallow, the raw TOML re-parse comparison) are
  attributed
  to the bounded test evidence's recorded context, not to direct source
  inspection.
- Optional native schemas are not referenced by `CONTRACT.md` for discovery
  and therefore count as no evidence.

## Arbitration required

The project architect owns arbitration and may request redesign and
implementation changes, explicitly approve an intent-document change, or
record a reasoned exception or approved deferral. This review does **not**
modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or `IMPL.md` to erase
any mismatch, and it does not treat external process success as acceptance.
The required decisions, in priority order, are:

1. **F-1 (HIGH):** Decide whether the workspace-namespace classification
   semantics (loaded/normalized `component_root`, root not emitted as a
   component, nested `kvist.toml` subprojects as regular components,
   section-independent classification) become declared intent
   (contract/requirements clause + revalidation) or receive a reasoned
   exception / approved deferral.
2. **F-4 (HIGH):** Decide how the observed discovery-hardening behavior is
   preserved in `IMPL.md` — a fresh clean-slate observation pass (then
   re-run this compliance pass) or a reasoned exception that the bounded
   tests are the retained observation.
3. **F-2 (MEDIUM):** Decide whether artifact-projection discovery-error
   propagation is declared in `CONTRACT.md`, excepted under the generic
   status-surfacing guarantee (C-5), or deferred.
4. **F-3 (LOW):** Decide whether single-parse classification is folded into
   the F-1 clause or needs no separate intent.
5. **F-5 (LOW):** No action beyond the normal verification + commit flow;
   recorded for state clarity only.

This record, the fresh `IMPL.md`, and any approved arbitration decision are to
be retained in version control. No raw provider transcript, credential,
secret, or temporary review workspace is retained.
