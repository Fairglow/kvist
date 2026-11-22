<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Settings Menu and Component Auto-Selection (`maerg`)

**Component:** `maerg`
**Task:** `settings-menu-compliance-review`
**Compared artifacts:**
- `maerg/REQUIREMENTS.md` §`REQ-SETTINGS-MENU`
- `maerg/CONTRACT.md` §`Provided interfaces` → "Markdown-first output and the
  interactive TUI" (the settings-menu / Esc–F2 / role-and-model clauses)
- `maerg/DESIGN.md` ("Settings menu", "Settings refinement", keyboard-shortcut
  help)
- `ROOT_CONTRACT.md` §"Change and compliance rules"
- `maerg/IMPL.md` clean-slate settings derivation ("Scoped settings
  observations - 2026-11-04", and the source-only supplement it points to at
  `docs/reviews/workspace-settings-observation.md`)
- bounded test evidence (listed below)

**Reviewer stance:** Source-blind, independent compliance reviewer. This record
is the disposition for the `settings-menu-compliance-review` gate; the
implementer and the security auditor did not author it, and it is not
self-certified.

## Review boundary and independence

This is the final stage of the `settings-menu` chain (tests → implementation →
security audit → compliance review). Per the runbook, the source-blind
compliance reviewer compares the intended artifacts with the implementation
record and bounded test evidence; the clean-slate observation of the surface
belongs to a separate context.

**Read (permitted intent context):** `maerg/REQUIREMENTS.md`,
`maerg/CONTRACT.md`, `maerg/DESIGN.md`, `ROOT_CONTRACT.md`,
`REVIEW_RUNBOOK.md` (the review procedure), and the canonical task-definition
projection for this chain in `maerg/TODOS.yaml`.

**Read (implementation record and bounded test evidence):** `maerg/IMPL.md`
(clean-slate settings sections only), the referenced clean-slate source-only
observation `docs/reviews/workspace-settings-observation.md` (the documenter's
derived record that `IMPL.md` points to), and the in-crate `#[cfg(test)]`
suites of `maerg/src/bark/tui/workspace/{menu_tests,model_menu_tests,
preferences,bindings}.rs`, `maerg/src/bark/prefill.rs`,
`maerg/src/bark/tui/workspace/view.rs`, and the integration evidence
`maerg/tests/tui_workspace.rs` (assertions only).

**Read to verify the record against current source (disclosed, not a boundary
breach):** the task directs that observed behavior be derived from source and
tests before the source-blind comparison. The retained clean-slate observation
predates this gate, so this reviewer additionally re-checked, against the
current `maerg/src` settings-menu paths, only the points that bear directly on
`REQ-SETTINGS-MENU` (idle-Esc dispatch, active-confirmation cancellation by F2,
role-clear profile-model guard, theme XDG path, and the active-command
refusal). Every "observed" statement below is attributed to the clean-slate
record, to the bounded test evidence, or to that disclosed verification; no
clause is certified from a prior compliance conclusion.

**Not read as evidence:** prior compliance reports (including the historical
`docs/reviews/workspace-settings-comparison.md`, whose F2 discrepancy and
active-preference boundary are recorded as historical), peer or parent
implementation artifacts, Git history/diffs, and the security-audit report of
this chain. The security audit's conclusions are not relied upon or repeated.

## Bounded evidence relied upon

- `IMPL.md` clean-slate settings sections: Esc/F2 menus retain parent
  cursor/filter and command drafts, distinguish current markers from selection,
  share the idle/active-confirmed Quit path, searchable component shortcuts
  persist privately with at most 12 pins and 12 recent entries, typed
  filesystem-only inspection supports all three roles with project/user scope,
  effort inheritance distinct from explicit `none`, model details and
  source-specific lifecycle actions, setup/check retain explicit host
  acknowledgement, and none of these settings approve protected execution. `IMPL.md`
  records the four original findings of the referenced observation (idle-Esc
  dispatch, role-clear profile mutation, theme XDG path, absent-user fallback
  disclosure) as repaired and independently reobserved.
- In-crate `menu_tests`: `escape_dismisses_completion_before_opening_the_menu_and_preserves_input`,
  `filtered_component_picker_and_pin_preserve_parent_and_command_draft`,
  `paste_is_routed_to_menu_search_and_never_mutates_hidden_command_input`,
  `quit_menu_uses_the_same_idle_exit_path`,
  `direct_theme_picker_returns_through_the_same_appearance_hierarchy`,
  `current_theme_marker_remains_when_highlight_moves`,
  `stale_cancel_never_reports_a_cancellation_or_changes_the_menu_selection`,
  `failed_focus_keeps_the_component_picker_and_command_draft`,
  `f2_opens_the_root_from_help_details_and_quit_without_changing_the_draft`,
  `f2_cancels_staged_confirmation_without_authorizing_or_retaining_its_input`,
  `active_commands_refuse_persisted_theme_and_shortcut_changes`,
  `quitting_active_work_requires_confirmation_and_reaps_the_owned_child`,
  `recalled_status_prefills_focus_and_observes_explicit_values_before_deletion`.
- In-crate `model_menu_tests`: `role_pages_start_in_user_scope_outside_an_initialized_project`,
  `all_roles_have_correct_current_effort_including_inherit_and_explicit_none`,
  `role_model_commands_pin_both_configuration_and_definition_and_keep_the_role`,
  `model_library_contains_disabled_profiles_and_inspection_is_offline`,
  `model_actions_pin_source_and_revision_and_stale_details_are_refused`,
  `referenced_model_actions_explain_required_unassignment_instead_of_attempting_mutation`,
  `scoped_role_picker_uses_effective_availability_not_shared_path_heuristics`,
  `first_project_edit_preselects_the_effective_effort_that_will_be_preserved`.
- In-crate `preferences::tests::shortcuts_are_bounded_private_and_restored`,
  `prefill.rs` tests (component prefill into the real Reedline buffer), and
  `view.rs` `theme_picker_preselects_current_value`.
- Integration evidence `tests/tui_workspace.rs`
  `bare_kvist_hosts_completion_reports_and_restorable_component_history`
  (Unix PTY): drives the **actual** F2 menu dispatch and asserts `.kvist/theme`
  becomes `light`, and `kvist.toml` gains `thinking_effort = "high"` and
  `profile = "fixture-b"` — i.e., real-terminal F2 selections of theme,
  developer model, and effort all persist.

This record is not execution proof. These suites are run automatically by
Kvist in the verification sandbox after this turn; no pass/fail count from
that run is asserted here.

## Method

Each obligation in `REQ-SETTINGS-MENU`, each contract clause in
`CONTRACT.md` `Provided interfaces` bearing on the settings menu / Esc–F2 /
role-and-model surface, the relevant `DESIGN.md` settings claims, and the root
`Change and compliance rules` were compared against the implementation record
and the bounded test evidence. Each item is classified **compliant**,
**mismatched**, **approved-deferred**, or **underspecified**, with component
path, locator, intended text, observed behavior/evidence, severity, and owner.

## Satisfied clauses (compliant)

**C-1 — The settings menu exposes the major choices (model, reasoning effort,
session history, component selection, theme, and other settings).**
`REQ-SETTINGS-MENU`; `CONTRACT.md`; `DESIGN.md`. Observed: the root page is
`Components`, `Role assignments`, `Models`, `Appearance`, `History`, `Help`, a
conditional `Cancel command` (only while a command runs), and `Quit`; the
`Role assignments` page reaches per-role `Model` and `Reasoning effort`
subpages; `Appearance` reaches `Theme` and a `Mask command answers` toggle. So
model (Models / Role assignments), effort (Role → Reasoning effort), session
history (History: follow + the newest segments), component selection
(Components), theme (Appearance → Theme), and other settings (Appearance
mask, Role assignments) are all exposed in the `skott` overlay pattern.
Compliant.

**C-2 — Selecting a component from the menu is the same focus change as `cd`
and pre-fills the component for subsequent commands.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`; `DESIGN.md`. Observed: the Components
picker dispatches `Action::Focus(Some(name))`, which calls `Workspace::change_focus`,
the identical function the `cd` builtin calls (`actions.rs` `"cd"` arm and
`menus.rs` `Action::Focus` both route to it). `change_focus` records the focus,
calls `prefill.begin_submission()`, and `ComponentPrefill` inserts the focused
component into the vacant component slot of subsequent commands (and the
dispatch path injects it into omitted component arguments), so the operator is
never asked for it. The menu path and the `cd` path share validation
(`ProjectContext::resolve_component`). Evidence:
`recalled_status_prefills_focus_and_observes_explicit_values_before_deletion`,
`filtered_component_picker_and_pin_preserve_parent_and_command_draft`,
`failed_focus_keeps_the_component_picker_and_command_draft`, and the PTY test
(`cd .`, `task ne<Tab>` → `task next .`). Compliant.

**C-3 — Theme, model, and effort selections persist the preference and apply
immediately to the session.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`; `DESIGN.md`. Observed:
- **Theme** — `set_theme` persists to `project/.kvist/theme` through
  `replace_file_atomically` and applies in place (`self.theme = theme`),
  guarded against an active command. Evidence: PTY test asserts `.kvist/theme`
  becomes `light`; `current_theme_marker_remains_when_highlight_moves`.
- **Model / effort** — the menu builds a pinned CLI command (`agent role set
  <role> <model> [--thinking-effort …] [--inherit-effort] --expected-revision
  … --profile-source … --profile-revision …`) that rewrites the configuration
  atomically (`wizard::set_role_model_pinned` → `replace_file_atomically`), and
  on command completion the workspace `refresh()`es state (status heading's
  `model:` is the configured developer profile). Evidence:
  `role_model_commands_pin_both_configuration_and_definition_and_keep_the_role`,
  `all_roles_have_correct_current_effort_including_inherit_and_explicit_none`,
  and the PTY test asserts `kvist.toml` gains `profile = "fixture-b"` and
  `thinking_effort = "high"` through the actual F2 menu dispatch.
The preference is persisted and reflected in the session; the mechanism for
model/effort is a pinned subprocess mutation plus a post-completion refresh
rather than a synchronous in-process state flip (see N-2). Compliant, subject
to N-2.

**C-4 — One hierarchical Esc/F2 menu with the required top-level pages and a
Cancel command while work is active.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: root = Components, Role
assignments, Models, Appearance, History, Help, `Cancel command` (present only
while `running` is set), and Quit. The menu is a single hierarchical stack
(`menu_stack`, `open_menu`/`back`). Compliant, with the superset note N-3
(conditional `Start agent session` / `Switch view` entries when idle/Skott
active).

**C-5 — Esc first dismisses completion, otherwise returns one menu level, and
opens the menu from the unobstructed workspace.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`; `DESIGN.md`. Observed: `Workspace::escape`
returns to `overlay_key(Esc)` (one menu level / dismisses the overlay) when an
overlay is open, else clears candidates (dismisses completion) when present,
else `open_menu(Page::Root)` from the workspace. Evidence:
`escape_dismisses_completion_before_opening_the_menu_and_preserves_input`.
Compliant.

**C-6 — Back-navigation restores the parent's cursor/filter without changing
settings or the command draft.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`; `DESIGN.md`. Observed: `open_menu` pushes
the parent chain and `back()` pops it; each parent page retains the
cursor/filter captured at open time; the command draft (`input.text`) is never
mutated by the menu — paste is routed to the searchable filter, and a failed
focus/action keeps the picker and the draft intact. Evidence:
`filtered_component_picker_and_pin_preserve_parent_and_command_draft`,
`paste_is_routed_to_menu_search_and_never_mutates_hidden_command_input`,
`failed_focus_keeps_the_component_picker_and_command_draft`. Compliant.

**C-7 — F2 opens the same root menu from every overlay and cancels a pending
confirmation without authorizing its action.**
`CONTRACT.md`; `DESIGN.md`. Observed: `overlay_key` checks `F(2)` first and
calls `open_menu(Page::Root)` for any overlay; for a staged `Confirm` it also
drops `pending_page` and the staged `prompt_file` and appends "Operation
cancelled". Evidence: `f2_opens_the_root_from_help_details_and_quit_without_changing_the_draft`
(Help, Details, Quit) and
`f2_cancels_staged_confirmation_without_authorizing_or_retaining_its_input`
(a later `y` starts no command). Compliant.

**C-8 — Quit and Ctrl+Q share cancellation/reaping and warn before terminating
active work.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: menu `Action::Quit` and the
`Ctrl+Q` handler both call `request_quit`; while a command runs this raises the
`Quit` confirmation, which on `y` cancels the owned command and reaps it, and
on Esc/`n` restores the prior overlay; idle Quit exits directly. Evidence:
`quitting_active_work_requires_confirmation_and_reaps_the_owned_child`,
`quit_menu_uses_the_same_idle_exit_path`. Compliant.

**C-9 — Ctrl+C remains cancel/clear (not quit); editing/completion, F1 help,
and transcript navigation retain their existing semantics.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: `Ctrl+C` cancels the owned command
while running, or clears input/candidates/overlay when idle; it never quits.
F1 opens Help; Tab/Shift+Tab and Enter keep completion semantics;
PageUp/PageDown, mouse wheel, and Ctrl+Home/End keep transcript-navigation
semantics. Compliant.

**C-10 — Pickers preselect and independently mark the current choice.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: `Menu::new` preselects the first
`current` entry; `Entry.current` marks the current value; the renderer prefixes
`[current]` independently of the highlighted cursor (`view.rs`). Evidence:
`current_theme_marker_remains_when_highlight_moves`,
`all_roles_have_correct_current_effort_including_inherit_and_explicit_none`,
`first_project_edit_preselects_the_effective_effort_that_will_be_preserved`,
`view.rs::theme_picker_preselects_current_value`. Compliant.

**C-11 — Component selection is searchable with bounded recent/pinned
shortcuts, not an unbounded tab strip.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: the Components page is
`searchable()` with case-insensitive substring filtering (256-byte bound), and
orders bounded pinned (≤12) and recent (≤12) shortcuts before the discovered
components, with `(project)` first and `[pinned]`/`[recent]` tags; there is no
per-component tab. Evidence:
`filtered_component_picker_and_pin_preserve_parent_and_command_draft`,
`paste_is_routed_to_menu_search_and_never_mutates_hidden_command_input`,
`preferences::shortcuts_are_bounded_private_and_restored`. Compliant.

**C-12 — Role settings show all three roles, effective model/effort, and their
configuration source.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: the `Role assignments` page lists
developer, architect, and security-reviewer as `<role>: <profile> | effort:
<value> | <scope>`, plus an "All assignments, providers and sources" details
page and per-role "Assignment sources and effective values". Compliant.

**C-13 — Editing a role does not change task-kind role selection or approve
policy.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: role edits dispatch a pinned
`agent role set/clear` command; confirmations and titles state that task kinds
and other role policies are unchanged and that "execution remains subject to
policy reapproval"; the page footer is "settings never approve policy".
`set_role_model_pinned` preserves the other roles' bindings/policies and
re-validates. Compliant.

**C-14 — Effort inheritance is distinct from explicit `none`.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: `override_effort:
Option<ReasoningEffort>` uses `None` = inherit and `Some(None)` = explicit
none; the effort page offers a "Default / inherit (profile: …)" entry plus an
explicit `none`. Evidence:
`all_roles_have_correct_current_effort_including_inherit_and_explicit_none`
(developer `high`, architect `Default / inherit`, security-reviewer `none`).
Compliant.

**C-15 — Unknown or unsupported capabilities are not presented as verified
support.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: effort values are labeled
"(support unverified)" when the profile's thinking support is unknown; a
profile that declares thinking unsupported does not expose explicit effort
choices at all; model details render `Thinking support: unknown` rather than an
inferred value. Compliant.

**C-16 — Active execution configuration stays pinned and settings mutations
wait until the owned command finishes.**
`REQ-SETTINGS-MENU`; `CONTRACT.md`. Observed: `setting_command`, `Action::Command`,
`change_focus`, `set_theme`, and the `Ctrl+P` pin all refuse while a command is
running; the role/model commands carry `--expected-revision` /
`--profile-source` / `--profile-revision` pins, and `set_role_model_pinned`
re-checks the revision and re-inspects the snapshot before writing, refusing a
stale selection. Read-only inspection and transcript navigation remain
available. Evidence:
`active_commands_refuse_persisted_theme_and_shortcut_changes`,
`stale_cancel_never_reports_a_cancellation_or_changes_the_menu_selection`,
`model_actions_pin_source_and_revision_and_stale_details_are_refused`.
Compliant.

**C-17 — Model inspection is local, credential-free, and shows provenance,
enabled state, backend/model, known capabilities, and references.**
`CONTRACT.md`. Observed: `model_settings::inspect*` reads only local TOML
configuration and registry files (bounded, non-following, symlink-traversal
refusing) and never contacts a provider; `details_markdown` shows state,
backend, provider, model, source/revision/scope, advertised/tested/
policy-enabled capabilities, thinking support, and known role references, with
empty evidence shown as "unknown (no recorded evidence)". Evidence:
`model_library_contains_disabled_profiles_and_inspection_is_offline`,
`scoped_role_picker_uses_effective_availability_not_shared_path_heuristics`.
Compliant.

**C-18 — Add/Check are explicit bounded host operations; acknowledgement is
separate from opening the menu.**
`CONTRACT.md`. Observed: "Add model" opens a y/n confirmation naming provider
discovery/qualification, host commands, and possible billing before running the
setup wizard; the two "Check" entries are labeled "explicit host
acknowledgement". Opening the menu performs no acknowledgement. Compliant.

**C-19 — Enable/Disable/Remove target the selected registry definition, refuse
stale definitions and current-project/user references, preserve unrelated
entries, and disclose shared-consumer/fallback effects before confirmation.**
`CONTRACT.md`. Observed: model mutations build pinned CLI commands with
`--registry`/`--expected-revision`; `Page::Model` refuses a definition whose
revision changed ("model definition changed; reopen Models"); a referenced
definition is presented as a Details "in use; unassign role references first"
rather than a mutation; the confirmation discloses known role references, the
exposed fallback, and that shared/user consumers cannot be enumerated. Evidence:
`model_actions_pin_source_and_revision_and_stale_details_are_refused`,
`referenced_model_actions_explain_required_unassignment_instead_of_attempting_mutation`.
Compliant.

**C-20 — The menu is a pure function of the session state, testable without a
terminal; the status heading reflects the selected component and model.**
`CONTRACT.md`; `DESIGN.md`. Observed: `build_menu(page: &self)` derives the page
from session state with no terminal dependency, and every in-crate test drives
`open_menu`/`overlay_key` against a `Workspace` built from a temp dir with no
terminal; the status heading shows the active component/state/progress plus
branch, sandbox, `model:` (configured developer profile), and live/stale locks,
with the selected component pre-filled. Compliant.

**C-21 — The root change-and-compliance process for this surface.**
`ROOT_CONTRACT.md`. Observed: the chain (→ tests → implementation → security
audit → compliance review) is reflected in the queue; this record retains its
findings for arbitration without altering `REQUIREMENTS.md`, `CONTRACT.md`,
`DESIGN.md`, `IMPL.md`, or the implementation. Compliant, subject to the
record-currency note N-1.

## Discrepancies and observations (retained for human arbitration)

No hard mismatch against `REQ-SETTINGS-MENU` or the `Provided interfaces`
contract was found. The items below are retained for explicit arbitration.

**N-1 (LOW, record-currency / evidence gap) — the clean-slate settings
observation retained in `IMPL.md` predates this gate, so this reviewer
re-verified the settings-menu-relevant points against current source.**
- *Locator:* `REVIEW_RUNBOOK.md` ("The documenter writes or proposes a fresh
  `IMPL.md` from observed evidence only"); `ROOT_CONTRACT.md` (clean-slate
  documenter + separate compliance reviewer); `IMPL.md` ("Scoped settings
  observations - 2026-11-04" and the referenced
  `docs/reviews/workspace-settings-observation.md`).
- *Intended:* the comparison rests on a clean-slate record of the surface under
  review, so clause-by-clause compliance can be attributed to the documenter
  context.
- *Observed:* the retained clean-slate settings record is the 2026-11-04
  supplement plus its `IMPL.md` summary; it was not re-dated for this task. It
  states the four original findings (idle-Esc dispatch, role-clear profile
  mutation, theme XDG path, absent-user fallback disclosure) were "repaired and
  independently reobserved." This reviewer could not write `IMPL.md` (component
  root, outside the writable `src/`/`tests/` scope) and, as the source-blind
  reviewer, did not re-derive the documenter's record. To keep the gate on
  current behavior, this reviewer verified, against the current
  `maerg/src` settings paths, exactly the points that bear on
  `REQ-SETTINGS-MENU`: idle-Esc dispatch (`workspace.rs::escape`), F2-from-
  overlay and staged-confirmation cancellation (`menus.rs::overlay_key`), the
  role-clear profile-model guard (`wizard.rs::clear_role_with_revision` now
  restricts the profile-model removal to role-named, legacy-shaped tables and
  refuses ambiguous tables), the theme XDG path (`theme.rs::user_config_file`
  now builds `…/kvist/<name>` without a duplicated `.config`), and the
  active-command refusal. All matched the record and the bounded tests.
- *Why it matters:* the runbook compares against a record; where the retained
  record predates this gate, clause compliance for the re-verified points rests
  on this reviewer's disclosed verification plus the bounded test evidence
  rather than on a freshly dated documenter record.
- *Arbitration options:* (a) accept this record, with the disclosed current-
  source verification, as closing the gate; (b) commission a separately dated
  clean-slate documenter pass to append a fresh `IMPL.md` settings observation
  and re-run this comparison; (c) record a reasoned exception that the 2026-
  11-04 clean-slate supplement and the bounded test evidence are the retained
  observation for this chain.

**N-2 (LOW, interpretation boundary) — "apply immediately to the session" for
model and effort is realized as a pinned subprocess mutation plus a post-
completion refresh, not a synchronous in-process state flip.**
- *Locator:* `REQ-SETTINGS-MENU` ("Theme, model, and effort selections MUST
  persist the preference and apply immediately to the session");
  `CONTRACT.md` ("Menus never silently modify an active command's pinned
  configuration"; role edits "apply to subsequent execution, never the active
  command"); `DESIGN.md` ("…apply immediately to the session. The menu is a
  pure function of the session state").
- *Intended:* a selection is persisted and takes effect for the session.
- *Observed:* theme applies in place (`set_theme` sets the live theme and
  persists atomically). Model and effort instead dispatch a pinned `agent role
  set` command that atomically rewrites the configuration; the preference is
  persisted at that point, and the session's visible state (the status
  heading's `model:`) is updated by `refresh()` when the command completes.
  There is a transient owned command between the selection and the refreshed
  state, and the change is not applied to any command that is active at the
  moment of selection (which the contract separately requires).
- *Why it matters:* the observable behavior (preference persisted; session
  reflects it for subsequent commands; status heading updates) satisfies the
  requirement, and the contract's own "subsequent execution, never the active
  command" wording reconciles the transient command. But "immediately" is read
  here as "persist now and reflect in session state once the mutation
  completes," not a synchronous in-process flip; that interpretation is a
  reasonable one but is not pinned by a dedicated test asserting the status
  heading updates after the menu-driven mutation.
- *Arbitration options:* (a) accept the interpretation and close; (b) add a
  bounded test that a menu-driven model/effort selection is reflected in the
  session state (e.g., the status heading's `model:`) after the mutation
  command completes; (c) clarify the intent wording to distinguish "applied to
  subsequent execution" from "synchronous in-process state change."

**N-3 (LOW, superset) — the root menu adds conditional entries beyond the
required list.**
- *Locator:* `REQ-SETTINGS-MENU` / `CONTRACT.md` (root pages).
- *Observed:* besides the required pages, the root page conditionally offers
  `Start agent session (Ctrl+A)` (idle, no active Skott session) and `Switch
  view (F3)` (active Skott session). All required entries are present; the
  additions are a superset.
- *Why it matters:* none functionally; noted so the superset is on the record
  rather than read as a missing/extra page.
- *Arbitration options:* accept as a deliberate superset, or, if the contract
  is meant to enumerate the root page exhaustively, update the contract to
  name the conditional entries.

**N-4 (LOW, approved-deferred limit, out of this requirement's primary scope)
— local-profile fallback disclosure is not proven complete when no user
configuration document exists.**
- *Locator:* `CONTRACT.md` (model library fallback disclosure); the referenced
  clean-slate observation's finding 4 (absent-user-source fallback blind spot);
  `IMPL.md` records it as reobserved.
- *Observed:* for a local (non-registry) profile, the exposed-fallback
  computation is driven by the inspected user-configuration documents; with no
  user document present, the built-in-default fallback may not be surfaced.
  This is a completeness limit of the model-library fallback disclosure and is
  primarily a `REQ-MODEL-MANAGEMENT` concern rather than a `REQ-SETTINGS-MENU`
  mismatch.
- *Why it matters:* it does not change any `REQ-SETTINGS-MENU` behavior; it is
  retained for awareness because the settings menu surfaces the same
  `details_markdown` fallback text.
- *Arbitration options:* accept as a documented limit; or, if complete fallback
  disclosure for local profiles is required, address it under the model-
  management surface.

## Discrepancy summary

| ID | Clause / finding | Status | Severity |
|----|------------------|--------|----------|
| C-1 | Menu exposes model, effort, history, component, theme, other | compliant | none |
| C-2 | Component selection = `cd` focus change; pre-fills commands | compliant | none |
| C-3 | Theme/model/effort persist and apply to the session | compliant (N-2) | low |
| C-4 | One hierarchical Esc/F2 menu + Cancel while active | compliant (N-3) | none |
| C-5 | Esc dismisses completion / returns a level / opens menu | compliant | none |
| C-6 | Back-navigation restores parent; draft preserved | compliant | none |
| C-7 | F2 from every overlay; cancels confirmation without authorizing | compliant | none |
| C-8 | Quit and Ctrl+Q share warning/cancel/reap | compliant | none |
| C-9 | Ctrl+C cancel/clear; F1/completion/transcript semantics retained | compliant | none |
| C-10 | Pickers preselect and independently mark current | compliant | none |
| C-11 | Searchable components with bounded pinned/recent shortcuts | compliant | none |
| C-12 | Role settings show all three roles, model/effort, source | compliant | none |
| C-13 | Role edits don't change task-kind selection or approve policy | compliant | none |
| C-14 | Effort inheritance distinct from explicit `none` | compliant | none |
| C-15 | Unknown/unsupported capabilities not shown as verified | compliant | none |
| C-16 | Active config pinned; mutations wait for command to finish | compliant | none |
| C-17 | Local, credential-free model inspection | compliant | none |
| C-18 | Add/Check are explicit bounded host operations | compliant | none |
| C-19 | Enable/Disable/Remove pin, refuse stale/referenced, disclose | compliant | none |
| C-20 | Menu is a pure function of session state; status heading | compliant | none |
| C-21 | Root change-and-compliance process | compliant (N-1) | none |
| N-1 | IMPL clean-slate record predates gate; current-source re-verified | record-currency | low |
| N-2 | "Apply immediately" = subprocess mutation + refresh (interpretation) | interpretation boundary | low |
| N-3 | Root menu superset entries (agent session / switch view) | superset | low |
| N-4 | Local-profile fallback disclosure completeness limit | approved-deferred | low |

**Total hard mismatches:** 0
**Observations retained for arbitration:** 4 (all LOW)

## Conclusion

The implemented settings menu and component auto-selection satisfy
`REQ-SETTINGS-MENU` and the `Provided interfaces` contract clauses bearing on
the surface. Every requirement and contract obligation compared is classified
**compliant**; no clause was found mismatched. Four LOW observations are
retained for explicit human arbitration (N-1 record currency and the disclosed
current-source re-verification, N-2 the "apply immediately" mechanism
interpretation for model/effort, N-3 the root-menu superset, and N-4 a
model-library fallback-disclosure completeness limit that is primarily
out-of-scope for this requirement). This record is the independent gate
disposition; it is not self-certified and does not by itself constitute
acceptance. Acceptance, if required, remains subject to the separate review
receipt / acknowledgement or a reasoned exception per the change-and-compliance
rules.
