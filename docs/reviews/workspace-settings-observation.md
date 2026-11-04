# Maerg workspace settings: source-only observation

Observed 2026-11-04. This is a fresh implementation supplement, not a
compliance, acceptance, or security certification. Evidence is restricted to
the requested Maerg source/tests and the two explicitly supplied execution
logs. No intent documents, previous implementation record, reviews, history,
or diffs were consulted. No source/tests were edited and no builds/tests were
run by this observer. References below are repository-relative.

## Concrete findings

1. **Idle Esc does not reach the menu-opening helper.**
   `maerg/src/bark/tui/workspace.rs:548-558` checks for `KeyCode::Esc`
   *inside* the branch requiring `KeyCode::Char('d')`. That check cannot
   succeed. The subsequent ordinary-key match has F2, but no Esc arm
   (`:571-631`). Consequently, idle Esc falls through to the generic input
   path rather than invoking `escape()` (`:635-644`); repeated idle Esc
   cannot open the root menu. Existing overlays do receive Esc through
   `overlay_key`. The helper-only test
   `maerg/src/bark/tui/workspace/menu_tests.rs:16-36` does not exercise this
   dispatch defect. The PTY test opens menus with F2, not idle Esc
   (`maerg/tests/tui_workspace.rs:191-218`).

2. **Role clearing can also change a canonical profile's model identifier.**
   After removing matching `agent.roles.*.profile` fields,
   `maerg/src/wizard.rs:1602-1621` additionally removes `model` from
   `agent.profiles.<role-name>`; `--all` applies this to every profile table.
   There is no format discriminator on that second loop. A valid local
   profile named `developer`, for example, can therefore lose its configured
   model when clearing the developer binding, even though it is a profile
   definition rather than a binding. The counter can also report a cleared
   assignment solely because that model field was removed. The inspected
   clear regression covers a shared-registry binding and retained effort/
   timeout, not this case (`maerg/tests/model_settings.rs:561-591`).

3. **Theme XDG paths contain an extra `.config` component.**
   Both `user_theme_path()` and `user_preference_path()` append `.config`
   after choosing either absolute `XDG_CONFIG_HOME` or `HOME`
   (`maerg/src/bark/theme.rs:439-464`). With `XDG_CONFIG_HOME=/home/u/config`,
   lookup is `/home/u/config/.config/kvist/{theme.toml,theme}`, not
   `/home/u/config/kvist/...`. This follows directly from the path-building
   code; it was not execution-tested here.

4. **Local fallback disclosure has an absent-user-source blind spot.**
   Local profiles initially receive `fallback: None`; their fallback
   calculation is nested inside the loop over existing user configuration
   documents (`maerg/src/model_settings/mod.rs:291-384`). Without a user
   document, the built-in-default lookup in that loop never runs, including
   for a local profile whose name matches a built-in default. Thus the
   inspection does not establish complete fallback disclosure in that case.
   The local fallback regression creates a user document
   (`maerg/tests/model_settings.rs:859-921`).

These are source-derived observations, not claims that a failing reproducer
was executed. No claim is made about when any finding was introduced.

## Implementation-derived supplement

### Interfaces and settings snapshots

`maerg/src/lib.rs:9-10,35` restricts the crate to Linux and publicly exports
`model_settings`. `maerg/src/model_settings/mod.rs:24-127,150-170` exposes:

- `Scope`, `Source { path, revision, scope }`, `RoleReference`,
  `FallbackDefinition`, `ProfileInspection`, `RoleInspection`,
  `RoleBindingInspection`, `SettingsInspection`, and `ProfilePin`.
- `inspect(project)`, `inspect_scope(project, global)`, and
  `inspect_with_registry(project, explicit_overlay)`.
- Re-exported `RegistryChange::{Enable,Disable,Remove}`, `MutationReport`,
  `mutate_registry`, and `remove_local_profile`.

Inspection reads project `kvist.toml`, `.kvist/config.toml`, user, and system
configuration inputs. Considered sources are validated even when shadowed.
User-scope inspection independently selects user configuration; ordinary
inspection delegates effective project resolution to configuration loading.
An explicit registry overlay changes inspection, not persisted role
authority (`maerg/src/model_settings/mod.rs:172-246`). Built-in execution
defaults are not automatically listed as configured models (`:247-289`).

Snapshots separate effective role values, raw scoped bindings, model
definitions, source identities, references, and fallbacks. Profiles sort by
name, effective-first, then source path. Known references include inspected
project/user inputs, including certain implicit bindings to configured
profiles; they are attached by profile name, not proven exclusive ownership
of a particular definition (`:386-460,537-569`). This is conservative usage
information, not an inventory of other projects.

Metadata presentation includes provider/model/backend, enabled/effective
state, source/revision, advertised/tested/policy-enabled capability lists,
thinking support, references, and known fallback. Empty evidence is
explicitly unknown. Details omit executable commands, arguments, prompts,
endpoints, and credentials; they distinguish native backend candidacy from
protected-task approval (`:60-80,833-895`). A local-command model without an
explicit model string is displayed using its profile name (`:264-272`).

### RGB palettes and rendering

Built-in palettes use explicit RGB SGR values, not remappable ANSI indices
(`maerg/src/bark/theme.rs:76-120`):

| Semantic color/surface | Dark RGB | Light RGB |
| --- | --- | --- |
| Canvas | 9,13,18 | 255,255,255 |
| Main text | 241,245,249 | 17,24,39 |
| Raised panel | 24,33,43 | 232,238,245 |
| Selected background | 49,85,129 | 29,78,216 |
| Selected text | 241,245,249 | 255,255,255 |
| Border | 111,131,153 | 100,116,139 |
| Secondary text | 166,180,197 | 71,85,105 |
| Red / failure | 252,165,165 | 185,28,28 |
| Green / indicator | 134,239,172 | 22,101,52 |
| Yellow | 253,224,71 | 146,64,14 |
| Blue / prompt | 125,183,255 | 29,78,216 |
| Magenta / component | 216,180,254 | 126,34,206 |
| Cyan | 103,232,249 | 14,116,144 |

Prompt/component/failure/indicator roles add bold. Secondary text uses its
own color, without the DIM modifier. Renderer helpers derive canvas, panel,
selection, secondary, and border styles from these roles
(`maerg/src/bark/tui/render.rs:110-135`). Markdown delegates to Blad,
translates output into Ratatui spans through an SGR parser, and supplies
theme-derived heading/link/math colors and raised code/table/quote surfaces
(`:28-102,145-181`).

The contrast test uses linearized sRGB relative luminance and
`(lighter + .05)/(darker + .05)`. It asserts at least **4.5:1** for main and
secondary text on canvas/panel, blue on canvas, all six status/accent colors
on canvas/panel, and selected text on selection background; border contrast
is at least **3:1** on canvas/panel. It also asserts distinct panel/canvas
colors and no secondary DIM modifier
(`maerg/src/bark/tui/render.rs:187-249`). These are test assertions, not a
runtime validator or certification of arbitrary custom themes, syntax
highlighting colors, terminal rendering, or all accessibility properties.

Theme resolution prioritizes color-disabling environment variables, then
environment/project/user preference and background detection. Theme changes
load/validate before saving `.kvist/theme` and changing the workspace theme;
failure leaves the previous selected theme/preference unchanged in the
tested missing-file case (`maerg/src/bark/theme.rs:284-313,480-544`;
`maerg/src/bark/tui/workspace/actions.rs:337-360`;
`maerg/src/bark/tui/workspace/view.rs:498-523`). Custom specs are bounded to
1,536 bytes and support 17 named roles, dark/light bases, hex, named colors,
and indexed colors (`maerg/src/bark/theme.rs:28,568-758`).

### Hierarchical menus, current markers, documents, and Quit

The root contains Components, Role assignments, Models, Appearance, History,
Help, conditional Cancel command, and Quit. Typed pages/actions maintain a
parent menu stack with an eight-level Open-action bound. Esc in an overlay
returns one level; F2 in a menu rebuilds the root. Direct Theme entry creates
the Appearance/root ancestry. Parent menu cursor/filter state and command
draft survive ordinary back navigation
(`maerg/src/bark/tui/workspace/menus.rs:20-140,155-285,382-397,487-512`).
The idle-Esc dispatch exception is finding 1. F2 is not handled as root
navigation inside Help, Details, or Confirm document overlays (`:333-381`).

Selection initially prefers the first `Entry.current`; rendering prefixes
all current entries with `[current]` independently of the highlighted cursor.
Theme, component focus, history follow/segment, role-model, and effort
choices use this mechanism (`maerg/src/bark/tui/workspace/menus.rs:100-112,
181-240`; `maerg/src/bark/tui/workspace/view.rs:232-267`;
`maerg/src/bark/tui/workspace/model_menus.rs:393-522`).

Component/model lists support case-insensitive substring filtering, a
256-UTF-8-byte query bound, sanitized paste, and grapheme-aware Backspace.
Nonsearchable overlays do not receive pasted text as hidden command input
(`maerg/src/bark/tui/workspace/menus.rs:114-140,447-475`;
`maerg/src/bark/tui/workspace.rs:646-663`).

Help, model/role Details, and confirmations use the same rendered Markdown
document viewport. Up/Down move one line; PgUp/PgDn use the transcript
viewport height; Home/End reach the ends. Drawing clamps and writes back the
scroll offset, so Up works after End. Mouse-wheel events produce three
Up/Down overlay events (`maerg/src/bark/tui/workspace/menus.rs:142-151,
333-381`; `maerg/src/bark/tui/workspace/view.rs:313-339`;
`maerg/src/bark/tui/workspace.rs:198-211`). The long-confirmation test checks
that a bottom consumer warning becomes visible and End then Up moves back
one line (`maerg/src/bark/tui/workspace/view.rs:347-388`).

Idle Quit sets the exit flag. Active Quit requires y/n/Esc confirmation;
declining restores the previous overlay, accepting requests cancellation.
The event loop exits only after the running slot is empty. Ctrl+C cancels
active work or clears idle input rather than quitting; Ctrl+D on empty input
sends child EOF or exits when idle. Cancellation messages explicitly deny
rollback (`maerg/src/bark/tui/workspace/menus.rs:275-285,316-332`;
`maerg/src/bark/tui/workspace.rs:184-186,492-525,529-559`).
The owned-child reaping behavior has a bounded library regression
(`maerg/src/bark/tui/workspace/menu_tests.rs:176-201`).

### Private shortcuts and transcript state

Project-wide component shortcuts persist separately from role/task settings
in `.kvist/transcript/workspace.json`. Pins and recents each allow at most
12 relative paths; entries must be nonempty, at most 4,096 bytes, free of
controls, and contain no parent/root/prefix path component. JSON denies
unknown fields and is limited to 16,384 bytes. Visits deduplicate/move to
front and truncate recents. Ctrl+P toggles pins and refuses a thirteenth;
the picker orders pins, recents, then discovered components, skipping stale
names and duplicates (`maerg/src/bark/tui/workspace/preferences.rs:8-72`;
`maerg/src/bark/tui/workspace/menus.rs:173-207,416-446`).

Private storage creates directories with mode 0700 and files with mode 0600;
existing files must be regular, single-link, and have no group/other mode
bits. Directory traversal rejects links/parent traversal; file opens use
nonfollowing flags. Existing directory modes are not tightened
(`maerg/src/bark/tui/storage.rs:11-72`). Preferences save through the atomic
replacement helper after validating any existing private file.

Focus changes reopen scope-local Markdown transcript and command history,
persist project recents, reset the input/history cursor, and refresh state;
they are refused during active execution
(`maerg/src/bark/tui/workspace/actions.rs:281-334`). Transcript segments
target 1 MiB and allow individual Markdown blocks up to 16 MiB, with no total
history retention limit; command history is bounded to 500 entries and
1 MiB. History menus list the latest 100 segments, while transcript
navigation can reach older segments (`maerg/src/bark/tui/storage.rs:8-9,
161-271`; `maerg/src/bark/tui/workspace/menus.rs:223-241`;
`maerg/src/bark/tui/workspace.rs:411-450`). These files are local diagnostic
state, not canonical workflow evidence.

### Role scope, model selection, and reasoning effort

Roles are developer, architect, and security-reviewer. Role pages switch
project/user edit scope without changing task-kind role selection. Model
choices include enabled, effective definitions only and pin both the role
configuration revision, when it exists, and the selected definition's
source/revision. External/local-command choices receive a warning rather
than being presented as native protected-task executors
(`maerg/src/bark/tui/workspace/model_menus.rs:39-110,302-439`).

`Default / inherit` removes only the role effort override. Explicit `none`
is a different value. The picker offers none/minimal/low/medium/high/xhigh/max
when support is true or unknown and the profile is enabled; unknown support
is labeled unverified. Known unsupported models do not expose explicit
effort choices. Existing current choices return without a mutation
(`maerg/src/bark/tui/workspace/model_menus.rs:440-529`).

`set_role_model_pinned` locks the target, validates configuration/model
scope, refuses unavailable/disabled selections and stale pins, preserves a
previous role override when neither effort option is supplied, and rejects
retained non-none effort on a model declaring thinking unsupported. It
changes profile/effort fields, validates resulting configuration, reinspects
the snapshot, rechecks any target revision, then writes atomically
(`maerg/src/wizard.rs:1269-1450`). Effort support marked unknown is not a
provider-specific verification of each offered effort value.

A first project edit inheriting another source preserves all three effective
role bindings and timeout/output/token policies, retains compatible registry
selection, and reports the resulting project precedence. It refuses to copy
sensitive inherited redaction values or silently change inherited registry
definitions (`maerg/src/wizard.rs:1460-1543`). Clear ordinarily removes the
binding's profile while retaining effort/resource fields, subject to the
additional profile-model mutation in finding 2 (`:1558-1659`).

### Model lifecycle, source revisions, references, and fallbacks

Registry inspection layers user, project, and explicit definitions in that
order, retaining the winning definition and immediate lower fallback.
It bounds merged distinct names to 128, includes disabled winners, and
keeps inactive user-scope definitions visible where separately resolved.
Registry model thinking support/default effort are currently unknown in
this projection, even for native models
(`maerg/src/model_settings/mod.rs:600-719`). Lower layers are not a complete
independently editable inventory in the model list.

Mutation requires exact registry path and revision, checks source membership,
locks target and known role-source inputs, and refuses disable/remove while
known references remain; enable does not require unassignment. It edits
only `enabled` or the selected profile entry, reparses, reinspects the entire
snapshot and source bytes, then calls atomic replacement. Reports disclose
previous/new revisions, actual source, fallback, and unknown external
consumers (`maerg/src/model_settings/storage.rs:18-145`). Revision checks use
`sha256:` over complete source text; registry inspection takes its digest
from the parsed resolved definition (`maerg/src/model_settings/mod.rs:699-719,
791-811`). Comment-only changes invalidate a selected model menu in the
library regression (`maerg/src/bark/tui/workspace/model_menu_tests.rs:133-179`).

Pinned local removal similarly refuses stale sources/references, removes
only the selected canonical profile, retains provider/unrelated tables,
validates configuration, and rechecks snapshot/source bytes
(`maerg/src/model_settings/storage.rs:149-232`). Local profiles have no
invented enable bit: their menu explains that enable/disable requires a
registry-backed profile (`maerg/src/bark/tui/workspace/model_menus.rs:253-293`).
Fallbacks describe known provider/model/source/revision/enabled state;
unknown consumers remain explicitly unenumerated. Finding 4 limits local
fallback completeness.

Path checking refuses symlink traversal, including ancestors, and delegates
bounded regular-file reads to the shared file helper
(`maerg/src/model_settings/mod.rs:753-789`). Settings sidecar locks use
create-new/nonfollowing opens and retry for at most 250 ms, then fail without
automatically deleting another lock; Drop removes the owned sidecar
(`maerg/src/model_settings/storage.rs:240-300`). These are cooperative locks,
not protection against arbitrary writers after the final check. Setup
binding persistence and older unpinned local remove/remove-all helpers do not
take these locks (`maerg/src/wizard.rs:239-299,433-615`), so the protection
must not be described as universal across all Kvist settings-writing paths.

### CLI parity, completion, and execution boundaries

The CLI declares agent setup/list/show/enable/disable/remove/check and
role list/set/clear. Enable/disable require registry path and revision;
registry removal requires revision and conflicts with `--all`. Role set
supports mutually exclusive explicit/inherited effort, optional configuration
revision, paired profile-source/profile-revision pins, and `--global`
(`maerg/src/cli.rs:347-472`). Human and JSON dispatch share role helpers and
model lifecycle helpers. JSON inspection/mutation includes structured data;
human details are Markdown (`:904-1025,1541-1696,2430-2518`).

Explicit registry path is the mutation target; `--global` is accepted for
enable/disable but is not separately enforced in their helper dispatch
(`maerg/src/cli.rs:2458-2475`). Outside a project, role/remove/check commands
fall back to user scope (`:2578-2600`). Unpinned local removal and `--all`
remain distinct routes; `--all` resets this configuration's registry/profile
bindings, not shared registry definitions
(`:2520-2576`; `maerg/src/wizard.rs:557-615`).

Completion's command tree is synthesized from Clap, with workspace builtins
added. Model-name candidates come from the inspection snapshot, sorted and
deduplicated, including disabled definitions for management; metadata
failure warns and leaves completion unavailable. Model loading is gated on
project `kvist.toml` existence
(`maerg/src/bark/tree.rs:125-145,631-648`;
`maerg/src/bark/state.rs:240-258,707-734`). The status model label uses the
configured developer profile, not a claim about a currently running model
(`maerg/src/bark/status.rs:63-83`).

Settings menus construct ordinary CLI argv, not shell command interpolation,
append a settings-command transcript entry, and launch the shared subprocess
path. Active execution blocks settings commands and component switching.
After child completion the workspace refreshes state and can rebuild the
requested settings page (`maerg/src/bark/tui/workspace/menus.rs:287-314,
562-585`; `maerg/src/bark/tui/workspace/actions.rs:264-286`;
`maerg/src/bark/tui/workspace.rs:492-525`). Read-only menus and appearance/
shortcut changes remain available; the block is not a global ban on all UI
preferences or external file writers.

Add model has a y/n menu warning about host discovery/qualification and
billing, then calls the setup wizard, which delegates to Sav setup when
needed. Check project/user commands have no additional menu confirmation
because `check_agents` itself requires affirmative host acknowledgement
before shared qualification or local test-command execution
(`maerg/src/bark/tui/workspace/model_menus.rs:214-227`;
`maerg/src/wizard.rs:175-193,1028-1198`). This observation does not independently
establish Sav subprocess/network bounds. Qualification/capability evidence
is presented separately from task approval. Embedded prompt dispatch
explicitly refuses `--allow-host-execution`
(`maerg/src/bark/tui/workspace/actions.rs:210-222`).

## Verification evidence and remaining gaps

**Inspected assertions, not execution by this observer:**

- `maerg/tests/model_settings.rs:84-958` contains 22 tests covering typed
  disabled-profile inspection, source-specific lifecycle and stale refusals,
  references, registry/local fallback, project/user scope, effort inheritance
  versus explicit none, unsupported retained effort, malformed/oversized/
  link/nonregular inputs, busy locks, preserved policy/comments, profile pins,
  and first-project inheritance/redaction refusal.
- Menu/model/view library tests assert navigation/current markers/filtering,
  private pins, offline inspection, stale model pages, reference explanations,
  long-document scrolling, theme failure preservation, and active Quit.
  Their helper calls do not establish idle Esc event dispatch.
- `maerg/tests/tui_workspace.rs:65-276` contains one real-PTY scenario covering
  F2-driven theme/model/effort mutations, child reports, masked/nonretained
  input, focus-local history, completion, transcript browsing, menu Quit, and
  alternate-screen restoration.
- `maerg/tests/markdown_output.rs:12-82` contains three tests for Markdown
  human output and preserved text/JSON/completion machine routes.

**Parent-executed logs actually read:**

- `/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/settings-bark-final.log`:
  263 passed, 0 failed, 386 filtered out.
- `/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/settings-targeted-final.log`:
  successive groups of 3, 22, and 1 passed, no failures. The parent identifies
  these as Markdown output, model settings, and the real-PTY scenario.

The supplied quiet logs are aggregate results, not source-hashed verification
receipts or proof of the untested cases above. The parent reports strict
all-target/all-feature Maerg Clippy passed, but no corresponding log was
provided or read. Full-workspace validation was described as in progress;
no full-workspace result is asserted here. No new reproducer, live provider
qualification, adversarial concurrent-writer test, or cross-terminal visual
check was executed for this observation.

## Correction/reobservation addendum - 2026-11-04, 08:56 CET

The four findings above describe the earlier observed source and remain
historical. The following independently reobserved source/tests supersede
their statements about the current implementation. This follow-up retained
the original exclusions, read only the relevant permitted source/tests and
newly allowed logs, and ran no commands, builds, or tests. Only this report
was edited. This remains observation, not certification or acceptance.

### Reobserved corrections

1. **Actual Esc dispatch is repaired.**
   `maerg/src/bark/tui/workspace.rs:548-563` now completes the empty-input
   Ctrl+D branch before checking Esc independently and routing it through
   `escape()`. The helper still backs out of an overlay, dismisses completion
   first, or opens the root when neither is present (`:635-644`).
   `maerg/tests/tui_workspace.rs:58-63` sends actual standalone Esc bytes;
   the scenario now uses idle Esc to open Appearance (`:202-207`) and the
   final Quit menu (`:257-258`), exercising dispatch rather than directly
   calling the helper. F2 remains exercised for role/model menus.

2. **Canonical role-named model definitions are preserved by clear.**
   `maerg/src/wizard.rs:1602-1654` no longer treats a role-like profile name
   alone as evidence of a role assignment. The additional profile-table
   clearing path requires a recognized role name, a model field, and at
   least one legacy-shape field: `command_template`, `models`, `token_limit`,
   or `timeout_seconds`. Without that legacy shape it skips the model field.
   If canonical fields such as provider/command/thinking metadata coexist
   with the legacy shape, it refuses the ambiguous table before writing.
   Validation, source-byte recheck, and replacement follow the complete
   in-memory edit (`:1668-1680`).

   `maerg/tests/model_settings.rs:960-999` asserts that both targeted clear
   and `--all` leave canonical developer-named and unrelated profile tables
   unchanged while clearing the selected bindings and retaining effort.
   `:1001-1011` asserts that a canonical developer profile without a binding
   produces refusal with unchanged bytes. `:1048-1075` covers successful
   unambiguous legacy clearing and byte-preserving ambiguous `--all` refusal.
   This is an explicit shape discriminator, not removal of legacy support.

3. **XDG theme/preference lookup is repaired and exercised at startup.**
   `maerg/src/bark/theme.rs:437-457` shares `user_config_file`: absolute
   `XDG_CONFIG_HOME` is used directly; otherwise HOME gains `.config`.
   Both spec and preference append `kvist/<filename>` to that base.
   The PTY fixture writes a named custom spec and matching preference under
   `user-config/kvist` (`maerg/tests/tui_workspace.rs:90-97`), removes the
   `KVIST_THEME` override, and sets XDG configuration home (`:147-154`).
   It waits for the actual `48;2;18;52;86` background sequence (`:170-179`),
   corresponding to the custom `#123456` canvas. Thus its assertion requires
   both the XDG spec and named preference to affect terminal output.

4. **Absent-user local fallback calculation is repaired and narrowed.**
   The fallback pass now follows, rather than nests within, user-source
   enumeration (`maerg/src/model_settings/mod.rs:342-392`). A built-in
   same-name fallback is reported only when the selected source declares
   exactly one local profile, so deleting that final definition can expose
   defaults. A root project definition can otherwise report a same-name
   local user definition. The source no longer conditions the whole pass on
   existence of a user document.

   `maerg/tests/model_settings.rs:1013-1046` explicitly establishes absent
   user configuration, asserts built-in `claude-default` fallback metadata
   and disclosure for the sole-profile case, then adds an unrelated profile
   and asserts null fallback. This regression checks inspection/disclosure;
   it does not itself execute removal and inspect the resulting execution
   configuration. That narrower executed-assertion boundary is retained.

### Related bounded observations

- Roles now selects user edit scope when root `kvist.toml` does not exist
  (`maerg/src/bark/tui/workspace/model_menus.rs:178-192`), rather than routing
  the initial role page to the unavailable project target. The no-project
  library regression asserts the developer entry opens with `global: true`
  (`maerg/src/bark/tui/workspace/model_menu_tests.rs:42-59`). This observes
  initial edit-scope selection, not an end-to-end first-user-file write.
- Model completion now proceeds when either root `kvist.toml` **or**
  `.kvist/config.toml` is a file
  (`maerg/src/bark/state.rs:240-262`). The earlier supplement's root-only
  gating description is superseded. No new dedicated `.kvist/config.toml`
  completion execution assertion was identified in this bounded reread.

No remaining concrete instance of the four historical defects was identified
in these repaired paths. This is not a new audit of the full implementation;
other limitations recorded above are not silently discharged by these fixes.

### New execution evidence and limits

The following are **parent-executed results**, not execution by this observer.
Paths share
`/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/`:

- `settings-maerg-library-final.log:433` reports **649 passed, 0 failed,
  1 ignored, 0 filtered out**, from 650 total library tests. The parent
  identifies this run as concurrent/all-feature and the ignore as existing;
  the quiet result does not identify the ignored test.
- The newer contents of `settings-targeted-final.log` report six successful
  groups: **8, 1, 3, 26, 1, and 13 passed**, with no failures or ignores.
  The parent identifies the selected suites as agent_check,
  config_resolution, Markdown output, model_settings, real PTY, and wizard.
  The earlier 22-test account above remains historical evidence from the
  previous contents of this same log path.
- `settings-clippy-final.log:1-2` shows Kvist checking and successful
  completion. The parent identifies the invocation as strict
  all-target/all-feature Maerg Clippy; the log does not include its command
  line, so those flags are parent-reported context rather than independently
  visible invocation evidence.

The parent separately reports successful formatting/diff checks, a repaired
queue-order issue with five continuous tests passing, and an earlier full
workspace run with two queue-order failures plus Blad deadline and checkpoint
index-lock failures. The parent describes the latter two as unchanged and
passing in isolation. Those checks, queue artifacts, failure sources, and
isolated results were not inspected here. No new full-workspace pass is
claimed; that validation is separate. Aggregate logs do not establish
source-hashed provenance, exhaustive concurrency behavior, or certification.

## Overlay/persistence reobservation — 2026-11-04, 09:11 CET

This additional source-only observation is limited to the changed overlay
dispatch, theme/pin persistence guards, their menu regressions, and the three
newly supplied log contents. Earlier observations remain historical. In
particular, the earlier statements that F2 is unhandled in document overlays
and that appearance/shortcut changes remain available during execution are
**superseded** below. No intent documents, implementation records, other
reviews, history, or diffs were consulted; no commands/tests were run and no
source/tests were edited.

### Current source-derived behavior

- **F2 centrally routes every overlay to Root.**
  `maerg/src/bark/tui/workspace/menus.rs:316-327` handles F2 before taking
  and matching the overlay. Consequently Help, Details, Confirm, Quit, and
  Menu share the same root-navigation path. `open_menu` replaces the parent
  stack and clears completion candidates without changing the command draft
  (`:245-269`); the F2 path also clears `quit_return`.
- **Leaving an active confirmation through F2 cancels, not authorizes.**
  The handler first recognizes `Overlay::Confirm`, opens Root, drops the
  retained authored-prompt file, clears `pending_page`, and appends
  `Operation cancelled.` (`:317-325`). It neither calls `start` nor executes
  the staged arguments; replacing the overlay discards that staged action.
  Cancellation cleanup precedes the fallible transcript append, so an append
  error would not restore or authorize the staged command.
- **Theme changes refuse an occupied running slot before side effects.**
  `maerg/src/bark/tui/workspace/actions.rs:337-343` returns an actionable
  error before preference classification, theme loading, directory creation,
  preference writes, or in-memory theme replacement. The guard applies to
  the shared `set_theme` helper rather than only one menu entry.
- **Pin/unpin refuses an occupied running slot before side effects.**
  Components Ctrl+P restores its current menu and returns an actionable
  error before changing cloned preferences or saving shortcuts
  (`maerg/src/bark/tui/workspace/menus.rs:421-460`). The guards inspect
  `running.is_some()`, so refusal lasts until the workspace has cleared that
  slot, not merely until the child might have exited externally.
- These guards do not prohibit opening/filtering/navigating menus.
  Read-only navigation remains available; this observation does not extend
  their protection to external writers or unrelated settings-writing paths.

### Inspected regression assertions

`maerg/src/bark/tui/workspace/menu_tests.rs:177-198` routes Help, Details,
and Quit through F2 and asserts Root, unchanged `status` draft, no quit,
and no running child. `:200-220` stages a confirmation with a retained
authored file and pending return page, then asserts Root, cleared pending
state, dropped/deleted file, and no child even after a subsequent `y`.
`:222-260` starts a command, attempts a different theme and Components
Ctrl+P, and asserts both refuse: theme identity and saved bytes are unchanged,
pins remain empty, no shortcut JSON file exists, and the Components menu
remains displayed. It then cancels/reaps the owned child with a deadline.
These are inspected test assertions, not tests executed by this observer.

No additional concrete defect was identified in this narrowly reread
dispatch/persistence boundary. The assertions do not establish exhaustive
overlay combinations, arbitrary filesystem-failure behavior, or protection
against non-workspace writers.

### Latest parent-executed evidence

The three log paths retain the session-files prefix recorded above; their
current contents supersede earlier aggregate counts:

- `settings-maerg-library-final.log:2,401`: **653 total; 652 passed,
  0 failed, 1 ignored, 0 filtered out**. All-feature/concurrent invocation
  and the ignore's pre-existing status are parent-reported context.
- `settings-targeted-final.log`: **52 passed** across successful groups
  of **8, 1, 3, 26, 1, and 13**, with no failures or ignores.
- `settings-clippy-final.log:1-2`: Kvist checking completed successfully;
  strict all-target/all-feature flags are identified by the parent, not
  printed in this short log.

The parent reports that the new regressions first failed before the fixes;
no pre-fix execution log was supplied for this follow-up, so that sequence
is not independently established here. A new full-workspace rerun is starting;
**no new full-workspace pass is claimed**. This addendum is neither an
acceptance decision nor a compliance/security certification.
