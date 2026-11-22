# Security Audit: Settings Menu

**Component:** maerg (Kvist interactive workspace)
**Scope:** Untrusted-input handling (model, theme, and effort selections validated against the configured set without injection), persistence (preferences written atomically without clobbering unrelated settings), and no authority change (the menu grants no new task or execution authority and component selection is the same focus change as `cd`).
**Method:** Source inspection of `src/bark/tui/workspace/{menus,model_menus,actions,preferences,menu_tests,model_menu_tests}.rs`, `src/bark/theme.rs`, `src/bark/tui/storage.rs`, `src/bark/tui/process.rs`, `src/bark/tui/editor.rs`, `src/model_settings/{mod,storage}.rs`, `src/wizard.rs`, `src/file_io.rs`, `src/context.rs`, and `src/cli.rs`, together with their in-crate `#[cfg(test)]` suites.

## 1. Untrusted-Input Handling

### 1.1 Model Selection — Satisfied

**Observation:** The `Page::RoleModel` menu (`model_menus.rs`) iterates over `snapshot.profiles` filtered by `candidate.enabled && candidate.effective`, so only enabled and effective profiles are shown. Each entry's `Action::Command` builds arguments via `role_arguments()` which:

- Uses the inspected `snapshot.sources` to find the matching source's revision for `--expected-revision`, pinning the configuration bytes.
- Inserts the profile name as a positional argument, taken from the inspected `ProfileInspection.name` field, not from user input.
- Appends `--profile-source` and `--profile-revision` from the inspected `ProfileInspection.source.path` and `source.revision`, pinning the model definition source.

The `set_role_model_pinned` function (`wizard.rs:1279`) re-validates the model name against the configured set: it checks `inspection.profiles.iter().any(|profile| profile.name == model_name && profile.enabled && ...)` and returns an error if the model is "disabled, unsupported, or not configured". It also validates the provider is available. The `expected_revision` pin prevents TOCTOU attacks: if the configuration bytes change between inspection and mutation, the revision check fails.

The `model_actions_pin_source_and_revision_and_stale_details_are_refused` test (`model_menu_tests.rs`) pins that a stale model definition (one whose revision has changed) is refused with an explicit error. The `role_model_commands_pin_both_configuration_and_definition_and_keep_the_role` test pins that the command arguments include both `--expected-revision` and `--profile-source`/`--profile-revision`.

**Assessment:** Strong. Model selection is validated against the configured set. The menu only shows enabled and effective profiles. The command arguments are built from the inspected snapshot, not from user input. The revision pin prevents TOCTOU attacks. No injection path exists.

### 1.2 Effort Selection — Satisfied

**Observation:** The `Page::Effort` menu (`model_menus.rs`) shows effort values from the `sav::ReasoningEffort` enum: `None`, `Minimal`, `Low`, `Medium`, `High`, `Xhigh`, `Max`. Each entry's `Action::Command` builds arguments via `role_arguments()` and appends `--thinking-effort <value>` where `<value>` is `value.as_str()` from the enum, not from user input. The `sav::ReasoningEffort::parse_effort` function (`cli.rs`) validates the effort value and returns an error for invalid values. The `set_role_model_pinned` function checks for conflicts between `--inherit-effort` and `--thinking-effort`.

The `all_roles_have_correct_current_effort_including_inherit_and_explicit_none` test (`model_menu_tests.rs`) pins that the current effort is correctly marked and that the "Default / inherit" entry uses `Action::Back` (no command) when the role has no override.

**Assessment:** Strong. Effort selection is validated against the `sav::ReasoningEffort` enum. The menu only shows valid effort values. The command arguments are built from the inspected snapshot. No injection path exists.

### 1.3 Theme Selection — Satisfied

**Observation:** The `Page::Theme` menu (`menus.rs`) iterates over `crate::bark::theme::registered()` which returns the registered themes (built-in and user-loaded). Each entry's `Action::Theme(name)` calls `self.set_theme(&theme)` where `name` is the registered theme name, not from user input.

The `set_theme` function (`actions.rs:762`) calls `bark::theme::match_preference(value)` to classify the value. If it's a path, it canonicalizes it. Then it calls `bark::theme::save_preference(&self.project, &value)` which validates the preference (built-in name, registered name, or absolute path) and writes it atomically via `replace_file_atomically`. The `match_preference` function (`theme.rs:413`) classifies the value: a value containing a path separator or ending in `.toml` is treated as a file path; `dark`/`light` (any case) select the built-ins; anything else is a theme name looked up in the registry. If the name is not registered, it falls back to a path.

The `save_preference` function (`theme.rs:475`) validates the preference: `matches!(matched, Preference::Dark | Preference::Light | Preference::Named(_)) || matches!(&matched, Preference::Path(p) if p.is_absolute())`. A relative path that is not a registered name is rejected with `KvistError::ThemePreferenceInvalid`. The write is atomic via `replace_file_atomically`.

The `set_theme` function also guards against a running command: `if self.running.is_some() { return Err(...) }`. The `current_theme_marker_remains_when_highlight_moves` test (`menu_tests.rs`) pins that the current theme marker is independent of the cursor position.

**Assessment:** Strong. Theme selection is validated against the registered set. The `save_preference` function validates the preference value. The write is atomic. No injection path exists.

### 1.4 Component Selection — Satisfied

**Observation:** The `Page::Components` menu (`menus.rs`) shows known components from `self.state.components()`, plus pinned and recent components from preferences. Each entry's `Action::Focus(Some(name))` calls `self.change_focus(focus)` where `name` is the known component name, not from user input.

The `change_focus` function (`actions.rs:706`) calls `project.resolve_component(Some(Path::new(component)))` which validates the component path. The `resolve_component` function (`context.rs:133`) normalizes the component argument, checks that the component is inside the project, and matches it against the inspected component list. If the component is not found, it returns `KvistError::ComponentNotResolvable` with the list of known components.

The `change_focus` function also guards against a running command: `if self.running.is_some() { return Err(...) }`. The `failed_focus_keeps_the_component_picker_and_command_draft` test (`menu_tests.rs`) pins that a failed focus change keeps the component picker and command draft intact.

**Assessment:** Strong. Component selection is validated against the known components. The `resolve_component` function validates the path. No injection path exists.

## 2. Persistence

### 2.1 Preferences (workspace.json) — Satisfied

**Observation:** `Preferences::save` (`preferences.rs`) validates the preferences before writing: max 12 entries per list, no empty entries, no entries > 4096 bytes, no control characters, no parent traversal (`Component::ParentDir` is rejected). The write is via `replace_file_atomically` which writes through a same-directory temporary file and atomically renames it, so the destination is never in a partially-written state. The `save` function also re-reads the existing file via `read_private` before replacing, so a concurrent writer that changed the file is detected.

`Preferences::load` (`preferences.rs`) reads via `read_private` (`storage.rs:55`) which uses `O_NOFOLLOW | O_NONBLOCK` and checks the file is a regular file with one link and mode 0o600. The read is bounded to 16384 bytes. The deserialized `Preferences` struct uses `#[serde(deny_unknown_fields)]` so unknown fields are rejected. The `validate` function is called after deserialization.

The `shortcuts_are_bounded_private_and_restored` test (`preferences.rs`) pins that pinned and recent components are bounded, that a path traversal entry (`../other`) is rejected, and that the previous valid state is preserved after a failed save.

**Assessment:** Strong. Preferences are written atomically via `replace_file_atomically`. The read is non-following and bounded. The validation prevents path traversal. No clobbering of unrelated settings.

### 2.2 Theme Preference — Satisfied

**Observation:** `save_preference` (`theme.rs:475`) validates the preference value before writing. The write is via `replace_file_atomically` which writes through a same-directory temporary file and atomically renames it. The `.kvist` directory is created via `private_directory` (`storage.rs:17`) which checks each ancestor is a non-link directory and creates missing ones with mode 0o700.

The `theme_spec_cap_is_the_documented_eight_kibibytes` test (`theme.rs`) pins the 8 KiB spec cap. The `theme_and_preference_reads_reject_fifos_and_bound_link_targets` test (`theme.rs`) pins that FIFOs and symlinks are rejected.

**Assessment:** Strong. Theme preference is written atomically. The validation prevents invalid values. No clobbering of unrelated settings.

## 3. No Authority Change

### 3.1 Menu Actions — Satisfied

**Observation:** The menu's `Action::Command` actions call `self.setting_command(arguments, return_to)` which calls `self.start(arguments)`. The `start` function (`actions.rs:630`) spawns a child process via `Running::spawn` (`process.rs:88`) which runs the current executable (`std::env::current_exe()`) with the given arguments. The child process is the same `kvist` binary, so it runs the same command-line interface with the same policy checks.

The confirmation prompts explicitly state that they do not authorize execution:

- `agent setup`: "This acknowledgement does not authorize later prompts or protected tasks."
- `agent check`: "explicit host acknowledgement" (no confirmation required for read-only checks).
- `agent role set`: "Execution remains subject to policy reapproval."
- `agent role clear`: "Task kinds and other role policies are unchanged; effective defaults may change. Execution remains subject to policy reapproval."
- `agent enable/disable/remove`: "This does not approve execution."
- `agent setup` (Add model): "Discovery/qualification executes bounded host commands or contacts configured providers and may incur billing. This acknowledgement does not authorize later prompts or protected tasks."

The `f2_cancels_staged_confirmation_without_authorizing_or_retaining_its_input` test (`menu_tests.rs`) pins that F2 cancels a staged confirmation without authorizing or retaining its input. The `stale_cancel_never_reports_a_cancellation_or_changes_the_menu_selection` test (`menu_tests.rs`) pins that a stale cancel (after the command has finished) does not report a cancellation or change the menu selection.

**Assessment:** Strong. The menu grants no new task or execution authority. All mutations require explicit confirmation. The confirmation prompts explicitly state that they do not authorize execution. The command arguments are built from the inspected snapshot, not from user input. The child process runs the same `kvist` binary with the same policy checks.

### 3.2 Component Selection — Satisfied

**Observation:** `change_focus` (`actions.rs:706`) changes the focus component. This is the same focus change as the `cd` builtin (`actions.rs:86`), which calls `self.change_focus(focus)` with the same validation. The `cd` builtin validates the component against `self.state.components()` and rejects unknown components. The `change_focus` function also saves preferences via `preferences.save(&self.project)`, which is the same persistence path as the menu's component selection.

The `filtered_component_picker_and_pin_preserve_parent_and_command_draft` test (`menu_tests.rs`) pins that the component picker and pin preserve the parent menu and command draft.

**Assessment:** Strong. Component selection is the same focus change as `cd`. It does not grant any new authority. The persistence path is the same as the menu's component selection.

## Findings

No findings. All three controls are satisfied:

1. **Untrusted-input handling:** Model, theme, and effort selections are validated against the configured set without injection. Component selection is validated against the known components.
2. **Persistence:** Preferences and theme preference are written atomically without clobbering unrelated settings.
3. **No authority change:** The menu grants no new task or execution authority. Component selection is the same focus change as `cd`.

## Required Fixes

None.

## Explicit Blockers

None.
