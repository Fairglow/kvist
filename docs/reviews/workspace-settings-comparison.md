# Maerg workspace settings: source-blind diagnostic comparison

Date: 2026-11-04.

## Scope and boundaries

This comparison reads only the settings/model/theme requirements and directly
related Markdown/workspace clauses in `maerg/REQUIREMENTS.md`, agent-command
and shell/TUI clauses in `maerg/CONTRACT.md`, and relevant settings,
palette/workspace sections of `maerg/DESIGN.md`. Implementation evidence is
limited to `workspace-settings-observation.md`, including its correction
addendum, and **only** the newest 2026-11-04 settings section of
`maerg/IMPL.md`. Execution evidence is the permitted parent-generated logs
listed below.

No implementation source, test source, queue, Git diff/history, checkpoints,
source-agent conversations, older IMPL sections, or unrelated reviews were
read. No builds/tests or agents were run. Source locations mentioned by the
observer were not independently opened here. This is diagnostic comparison,
not compliance certification, intent acceptance, a review receipt, or closure
of any protected queue gate.

## Intent versus available evidence

| Intent | Current evidence and comparison |
| --- | --- |
| Hierarchical menu, completion-first Esc, retained navigation/draft, independent current markers, shared confirmed Quit | The observation describes typed pages, saved cursor/filter ancestry, current markers distinct from highlight, scrollable detail/confirmation documents, and shared cancellation/reaping. The correction observes repaired **actual idle-Esc dispatch**, with inspected standalone-Esc PTY assertions. Broad agreement, except the F2 document-overlay discrepancy below. |
| Searchable components, bounded recent/pinned shortcuts, focus-local history | The observation describes private persistence with **12 pins and 12 recents**, validated paths, searchable choices, and reopening scope-local transcript/history on focus change. The inspected PTY scenario exercises focus-local history. No contradictory current behavior is reported. |
| Three roles, effective source/model/effort, separate edit scope, inheritance distinct from explicit `none`, no policy approval | Typed inspection and menus distinguish all three roles, project/user scope, known versus unknown support, inherited versus explicit effort, and protected-task approval. First-project edits preserve effective bindings/resource policies or refuse unsafe forks. The correction observes initial user scope outside a project and preservation of canonical role-named profiles during clear. These are source observations and inspected assertions, not independently executed reproductions here. |
| Offline, credential-free model inspection; acknowledged bounded Add/Check; source-specific lifecycle with stale/reference/fallback safeguards | The observation describes filesystem-only snapshots, actual sources/digests, credential-free details, explicit host acknowledgement, ordinary CLI argv dispatch, reference refusals and validated atomic writes. The correction observes absent-user fallback disclosure, narrowed to when deleting the final local definition can expose defaults. Inspection/disclosure is asserted; that regression does **not** itself execute removal and inspect resulting execution configuration. |
| Pinned execution configuration and deferred settings changes | The observation says active execution blocks model/role settings commands and component switching, with filesystem refresh after completion. Appearance and shortcut changes remain available. No active model/effort pinning violation is reported, but the broader requirement wording needs the boundary clarification below. |
| Independent RGB themes, semantic raised/selected surfaces, contrast and common Blad/UI palette | The observer reports 17 semantic roles, independent dark/light RGB tables and shared renderer colors. Inspected contrast assertions require **4.5:1** for tested text/accent combinations and **3:1** for borders. The correction observes repaired XDG spec/preference lookup and a PTY assertion for the custom startup canvas sequence. Theme failure preservation is inspected. These are not visual/accessibility certification or exhaustive verification of legacy theme behavior. |
| Markdown human output with preserved machine routes and terminal ownership | The observation describes Markdown details/results, CLI-derived completion and argv-based owned subprocesses. Inspected Markdown tests preserve text/JSON/completion routes; the PTY scenario covers terminal restoration. The bounded comparison does not re-audit the entire Markdown workspace. |

The observation's original four findings—idle Esc, canonical-profile clearing,
XDG lookup, and absent-user fallback—are **historical**. Its correction
supersedes them; they are not current repair findings.

## Current discrepancy and interpretation boundary

### D1 — F2 does not consistently open the unified menu

The current contract says “F2 opens that same menu”; the Settings refinement
design says F2 aliases the workspace menu. The observation's hierarchical-menu
section explicitly reports that F2 is **not handled as root navigation inside
Help, Details, or Confirm document overlays**. The correction repairs idle Esc
but does not supersede this separate observation.

On the supplied source-derived evidence, this remains a concrete dispatch
discrepancy with the unqualified F2 interaction promise. Repair the
document-overlay F2 route, preserving explicit confirmation semantics, or
submit an intentional contextual exception for human intent arbitration.
Passing aggregate logs do not establish coverage of this route. This reviewer
did not execute a reproducer and makes no broader navigation-failure claim.

### B1 — Scope of the active-command settings restriction

`REQ-SETTINGS-MENU` says “settings mutations MUST wait until the owned command
finishes.” The observation explicitly allows active appearance/shortcut
changes. The design more narrowly disables **model/effort** changes, and the
contract protects the active command's pinned configuration.

Thus a blanket reading of the requirement conflicts with the described UI
preference behavior, while an execution-configuration reading agrees with the
design/contract. This is an unresolved intent boundary, not evidence that an
active command's model/effort changes underneath it. Human arbitration should
clarify the intended scope; this comparison does not silently narrow the
requirement or demand blocking harmless presentation changes.

## Parent-executed validation evidence

All paths below are under
`/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/`.
These logs were read, but the commands were not executed by this reviewer.

| File | Visible result and attribution limits |
| --- | --- |
| `settings-workspace-nextest-final.log` | Run **8a998e13-24a9-45d7-b0fb-0eaaef974355**, default nextest profile, **2,301 tests across 113 binaries; 2,301 passed, 15 skipped, 1 slow**, summary duration **326.114s**. All-feature flags, two-test concurrency and the skips being existing are parent-reported context, not visible command-line evidence. |
| `settings-maerg-library-final.log` | **649 passed, 0 failed, 1 ignored, 0 measured, 0 filtered out**, **20.33s**: 650 library tests including the ignore. Concurrent/all-feature invocation and the ignore being existing are parent-reported. |
| `settings-targeted-final.log` | Six successful groups: **8, 1, 3, 26, 1, 13**, totaling **52 passed**, with **0 failed/ignored/measured/filtered out** in every group. The parent identifies agent-check, config-resolution, Markdown-output, model-settings, actual-PTY and wizard groups respectively; quiet output does not independently identify suites/test names. |
| `settings-clippy-final.log` | Kvist checking followed by successful dev-profile completion in **4.68s**. Strict all-target/all-feature Maerg lint flags are parent-reported; the log omits the invocation. |
| `settings-release-build.log` | Kvist compilation followed by successful release-profile completion in **29.86s**. |
| `settings-release-smoke.log` | Present: records valid structured role-list success JSON, valid Maerg component documents, and successful release bare Markdown/help flags, component validation, formatting and diff checks. It is a summarized parent-generated record, not full command/output provenance. |

The earlier `settings-workspace-nextest.log` was not needed or read; earlier
failures are not substituted for the final successful run. Historical counts
in the observation's original verification section are superseded by the
newer supplied logs. Counts above overlap and must not be added as unique
coverage. The parent also reports successful formatting/diff checks and
component document validation; no checks were repeated here.

## Remaining limits and disposition

Registry thinking/default-effort metadata can remain unknown; lower registry
layers are not a complete independently editable inventory; references cannot
enumerate external projects. Cooperative locks do not cover arbitrary writers
or every older unpinned setup/removal path. The observation does not
independently establish Sav subprocess/network bounds. No dedicated execution
assertion was identified for the newly widened `.kvist/config.toml` completion
gate or end-to-end first-user-file role persistence.

Contrast coverage is limited to inspected combinations, not arbitrary custom
themes, syntax colors, terminal behavior, or universal accessibility. Aggregate
logs are not source-hashed receipts, exhaustive race tests, or proof of every
intent clause. No live provider qualification was independently performed.

**Disposition:** bounded agreement on the repaired findings and the main
settings/model/theme behavior, with D1 requiring repair or explicit human
arbitration and B1 requiring scope clarification. This record neither certifies
compliance nor accepts intent, closes gates, or authorizes policy changes.

## Correction/comparison addendum — 2026-11-04, post-overlay/persistence fixes

This follow-up preserves the original comparison as historical. It reads only
the permitted current settings intent, newest settings IMPL section, final
**09:11 CET Overlay/persistence reobservation** in the source-only supplement,
and permitted parent-generated logs. The original source-blind exclusions
remain in force: no source/tests, Git, queue, conversations, checkpoints,
builds/tests or agents were consulted/run. Only this comparison is edited.

### D1 — Addressed on the supplied current evidence

The contract now explicitly specifies F2 from every overlay and cancellation,
not authorization, of a pending confirmation; the design describes the same
behavior. The independent observer reports centralized F2 handling before
overlay matching, covering Help, Details, Confirm, Quit and Menu. Root
navigation preserves the command draft, resets navigation/completion state,
and clears the retained quit-return state.

For staged confirmations, the observer reports discarding the staged action,
dropping the authored-prompt file and clearing the pending page before the
fallible cancellation-message append. Inspected regressions assert Root
navigation from document/Quit overlays and confirmation cleanup, including
that a subsequent `y` does not start a child. These source-derived observations
supersede the earlier contextual F2 limitation. No current D1 contradiction
is identified within this boundary; this reviewer did not execute those paths.

### B1 — Addressed conservatively, without narrowing the requirement

The broad `REQ-SETTINGS-MENU` restriction remains: settings mutations must wait
until the owned command finishes. Current Contract/Design explicitly prohibit
persisted settings changes, including appearance and component shortcuts,
during active execution, while retaining read-only inspection/navigation.

The observer now reports a shared theme-helper guard before loading, filesystem
changes or in-memory replacement, plus a pin/unpin guard before preference
mutation or persistence. Both refuse while the running slot is occupied,
including the interval before the workspace clears an exited child. Existing
model/role mutation and focus restrictions remain part of the earlier evidence.
Inspected regressions assert unchanged theme identity/preference bytes, empty
pins, absence of shortcut-file creation, and preservation of the Components
menu during refusal.

Thus the new behavior implements the broader requirement rather than treating
presentation persistence as an exception. The original B1 interpretation
boundary is historical, not a remaining request to weaken intent. No current
contradiction is identified in these observed workspace mutation paths.

### Post-correction parent-executed evidence

The following are current contents of the same permitted session-files paths,
not execution by this reviewer:

- `settings-maerg-library-final.log`: **653 total: 652 passed, 0 failed,
  1 ignored, 0 measured, 0 filtered out**, **17.76s**. All-feature/concurrent
  invocation and the ignore being existing remain parent-reported context.
- `settings-targeted-final.log`: **52 passed** across **8, 1, 3, 26, 1,
  13**; every group reports zero failures/ignores/measured/filtered out.
  Suite identities, including the actual PTY scenario, remain parent-attributed.
- `settings-clippy-final.log`: successful Kvist dev-profile checking in
  **5.50s**. Strict all-target/all-feature Maerg lint flags are parent-reported,
  not printed in the log.
- `settings-workspace-nextest-final.log` is **partial** at this read:
  run **4d20f182-7fb9-43d9-a043-4fb943c4e90e** starts **2,304 tests across
  113 binaries, 15 skipped**, without a result summary. No completed
  post-correction full-workspace pass is claimed.

The original **2,301-pass** workspace result and run
**8a998e13-24a9-45d7-b0fb-0eaaef974355** precede these final fixes and are
historical, not current full-run evidence. The earlier release-build/smoke
entries likewise do not establish validation of the post-correction state.
The release rebuild is parent-reported pending; no new release result is
claimed and none was awaited.

The parent reports three regressions demonstrated failing before the fixes
and passing afterward. The independent observer inspected their assertions
and final logs, but no pre-fix failure log was supplied here; the failing-first
sequence is therefore parent-reported, not independently established by this
comparison. Counts overlap and are not additive unique coverage.

**Updated disposition:** D1 and B1 are addressed on the bounded current
source-derived and parent-executed evidence; no additional concrete
contradiction is identified in this follow-up scope. Remaining limits include
non-workspace writers, nonexhaustive overlay/filesystem-failure coverage,
missing source-hashed provenance, and the original unrelated evidence gaps.
This is neither compliance certification nor intent acceptance, and it closes
no protected queue gate.
